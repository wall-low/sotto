// Внешние программы не лежат в установщике: при первом запуске они скачиваются
// в папку данных приложения. Так установщик маленький, а yt-dlp можно обновлять
// отдельно, YouTube ломает старые версии часто.
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use flate2::read::GzDecoder;
use serde::Serialize;
use sha2::{Digest, Sha256};

const VOT_VERSION: &str = "2.1.0";
const DENO_VERSION: &str = "2.9.8";
const FFMPEG_RELEASE: &str = "b6.1.1";
const YTDLP_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const YTDLP_UPDATE_PERIOD: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Copy)]
enum Pack {
    Raw,
    Gzip,
    Zip,
    TarGz,
}

struct Component {
    title: &'static str,
    file: &'static str,
    url: String,
    // None значит, что сумма берётся из SHA2-256SUMS свежего релиза
    sha256: Option<&'static str>,
    pack: Pack,
}

#[derive(Clone, Copy)]
struct Target {
    ytdlp: &'static str,
    vot: &'static str,
    vot_pack: Pack,
    vot_sha256: &'static str,
    ffmpeg: &'static str,
    ffmpeg_sha256: &'static str,
    deno: &'static str,
    deno_sha256: &'static str,
}

include!("target.rs");

fn component_file(stem: &'static str) -> &'static str {
    match (stem, cfg!(windows)) {
        ("yt-dlp", true) => "yt-dlp.exe",
        ("vot", true) => "vot.exe",
        ("ffmpeg", true) => "ffmpeg.exe",
        ("deno", true) => "deno.exe",
        _ => stem,
    }
}

fn components() -> Vec<Component> {
    let target = TARGET;
    vec![
        Component {
            title: "yt-dlp",
            file: component_file("yt-dlp"),
            url: format!("{YTDLP_BASE}/{}", target.ytdlp),
            sha256: None,
            pack: Pack::Raw,
        },
        Component {
            title: "ffmpeg",
            file: component_file("ffmpeg"),
            url: format!(
                "https://github.com/eugeneware/ffmpeg-static/releases/download/{FFMPEG_RELEASE}/{}",
                target.ffmpeg
            ),
            sha256: Some(target.ffmpeg_sha256),
            pack: Pack::Gzip,
        },
        Component {
            title: "озвучка",
            file: component_file("vot"),
            url: format!(
                "https://github.com/FOSWLY/vot-cli/releases/download/{VOT_VERSION}/{}",
                target.vot
            ),
            sha256: Some(target.vot_sha256),
            pack: target.vot_pack,
        },
        Component {
            title: "deno",
            file: component_file("deno"),
            url: format!(
                "https://github.com/denoland/deno/releases/download/v{DENO_VERSION}/{}",
                target.deno
            ),
            sha256: Some(target.deno_sha256),
            pack: Pack::Zip,
        },
    ]
}

pub fn tools_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SOTTO_TOOLS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Sotto")
        .join("tools")
}

pub fn tool_path(stem: &'static str) -> PathBuf {
    tools_dir().join(component_file(stem))
}

// Переменная PATH для запуска: vot-cli находит yt-dlp, а yt-dlp находит deno по ней
pub fn search_path() -> std::ffi::OsString {
    let mut paths = vec![tools_dir()];
    if let Some(system) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&system));
    }
    std::env::join_paths(paths).unwrap_or_default()
}

#[derive(Serialize, Clone)]
pub struct Status {
    pub ready: bool,
    pub missing: Vec<String>,
}

pub fn status() -> Status {
    let dir = tools_dir();
    let missing: Vec<String> = components()
        .into_iter()
        .filter(|component| !dir.join(component.file).is_file())
        .map(|component| component.title.to_string())
        .collect();
    Status {
        ready: missing.is_empty(),
        missing,
    }
}

#[derive(Serialize, Clone)]
pub struct SetupProgress {
    pub title: String,
    pub index: usize,
    pub count: usize,
    pub fraction: f64,
}

pub fn install(on_progress: impl Fn(SetupProgress)) -> Result<(), String> {
    let dir = tools_dir();
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let client = http_client()?;

    let pending: Vec<Component> = components()
        .into_iter()
        .filter(|component| !dir.join(component.file).is_file())
        .collect();
    let count = pending.len();

    for (index, component) in pending.iter().enumerate() {
        let expected = match component.sha256 {
            Some(sum) => sum.to_string(),
            None => ytdlp_checksum(&client)?,
        };
        install_component(&client, component, &expected, &dir, |fraction| {
            on_progress(SetupProgress {
                title: component.title.to_string(),
                index: index + 1,
                count,
                fraction,
            })
        })?;
    }
    Ok(())
}

// Вызывается при запуске приложения. Ошибка здесь не мешает работе со старой версией
pub fn update_ytdlp_if_stale() -> Result<bool, String> {
    let dir = tools_dir();
    let stamp = dir.join("yt-dlp.checked");
    let fresh = fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age < YTDLP_UPDATE_PERIOD);
    let path = tool_path("yt-dlp");
    if fresh || !path.is_file() {
        return Ok(false);
    }

    let client = http_client()?;
    let expected = ytdlp_checksum(&client)?;
    let updated = sha256_of(&path).map_err(|error| error.to_string())? != expected;
    if updated {
        let component = components().into_iter().next().ok_or("нет компонента")?;
        install_component(&client, &component, &expected, &dir, |_| {})?;
    }
    fs::write(stamp, b"").map_err(|error| error.to_string())?;
    Ok(updated)
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("Sotto")
        .connect_timeout(Duration::from_secs(30))
        .timeout(None)
        .build()
        .map_err(|error| error.to_string())
}

fn ytdlp_checksum(client: &reqwest::blocking::Client) -> Result<String, String> {
    let text = client
        .get(format!("{YTDLP_BASE}/SHA2-256SUMS"))
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.text())
        .map_err(|error| error.to_string())?;
    text.lines()
        .find_map(|line| {
            let (sum, name) = line.split_once(char::is_whitespace)?;
            (name.trim().trim_start_matches('*') == TARGET.ytdlp).then(|| sum.to_lowercase())
        })
        .ok_or_else(|| "в релизе yt-dlp нет контрольной суммы".to_string())
}

fn install_component(
    client: &reqwest::blocking::Client,
    component: &Component,
    expected: &str,
    dir: &Path,
    on_fraction: impl Fn(f64),
) -> Result<(), String> {
    let work = tempfile::tempdir_in(dir).map_err(|error| error.to_string())?;
    let download = work.path().join("download");

    let mut response = client
        .get(&component.url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("{}: {error}", component.title))?;
    let total = response.content_length().unwrap_or(0) as f64;

    let mut file = File::create(&download).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    let mut done = 0.0;
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("{}: {error}", component.title))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        done += read as f64;
        if total > 0.0 {
            on_fraction(done / total);
        }
    }
    drop(file);

    let actual = hex(&hasher.finalize());
    if actual != expected.to_lowercase() {
        return Err(format!("{}: контрольная сумма не совпала", component.title));
    }

    let unpacked = work.path().join("unpacked");
    unpack(&download, &unpacked, component.pack)
        .map_err(|error| format!("{}: {error}", component.title))?;
    make_executable(&unpacked).map_err(|error| error.to_string())?;
    fs::rename(&unpacked, dir.join(component.file)).map_err(|error| error.to_string())
}

// В архивах компонентов лежит один файл, поэтому берём первый
fn unpack(archive: &Path, out: &Path, pack: Pack) -> io::Result<()> {
    match pack {
        Pack::Raw => fs::copy(archive, out).map(|_| ()),
        Pack::Gzip => {
            let mut decoder = GzDecoder::new(File::open(archive)?);
            io::copy(&mut decoder, &mut File::create(out)?).map(|_| ())
        }
        Pack::TarGz => {
            let mut tar = tar::Archive::new(GzDecoder::new(File::open(archive)?));
            for entry in tar.entries()? {
                let mut entry = entry?;
                if entry.header().entry_type().is_file() {
                    io::copy(&mut entry, &mut File::create(out)?)?;
                    return Ok(());
                }
            }
            Err(io::Error::new(io::ErrorKind::NotFound, "пустой архив"))
        }
        Pack::Zip => {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index)?;
                if entry.is_file() {
                    io::copy(&mut entry, &mut File::create(out)?)?;
                    return Ok(());
                }
            }
            Err(io::Error::new(io::ErrorKind::NotFound, "пустой архив"))
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn sha256_of(path: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
