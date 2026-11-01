use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::errors::{self, Failure, Kind};
use crate::tools;

const ORIGINAL_VOLUME: &str = "0.2";
// Озвучка Яндекса всегда mp3 128 кбит/с, так что итоговый размер известен заранее
const VOICE_BYTES_PER_SECOND: f64 = 16_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dubbed,
    Video,
    Voice,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub link: String,
    pub mode: Mode,
    pub quality: String,
    pub folder: PathBuf,
    pub local_file: Option<PathBuf>,
    pub browser: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Plan { steps: Vec<&'static str> },
    Stage { step: &'static str, title: String, detail: Option<String> },
    Info { title: String, duration: Option<f64>, height: Option<u32>, thumbnail: Option<String> },
    Progress {
        fraction: Option<f64>,
        remaining: Option<f64>,
        speed: Option<f64>,
        done: Option<f64>,
        total: Option<f64>,
    },
    Browser { name: Option<String> },
    Log { line: String },
    Finished { path: PathBuf, size: u64 },
    Failed { failure: Failure },
    Cancelled,
}

#[derive(Default)]
pub struct Control {
    cancelled: AtomicBool,
    running: Mutex<Option<u32>>,
}

impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.running.lock().unwrap() {
            kill_tree(pid);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
fn prepare(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Своя группа процессов, чтобы отмена досталась и потомкам
    command.process_group(0);
}

#[cfg(windows)]
fn prepare(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn kill_tree(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
}

#[cfg(windows)]
fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000)
        .output();
}

enum Stop {
    Failed(Failure),
    Cancelled,
}

type Outcome<T> = Result<T, Stop>;

struct Metadata {
    title: String,
    duration: Option<f64>,
    height: Option<u32>,
    size: Option<f64>,
    thumbnail: Option<String>,
    live_status: String,
}

struct Runner<'a> {
    request: &'a Request,
    control: &'a Control,
    emit: &'a (dyn Fn(Event) + Sync),
    log: Mutex<Vec<String>>,
}

pub fn run(request: &Request, control: &Control, emit: &(dyn Fn(Event) + Sync)) {
    let runner = Runner { request, control, emit, log: Mutex::new(Vec::new()) };
    match runner.execute() {
        Ok((path, size)) => emit(Event::Finished { path, size }),
        Err(Stop::Cancelled) => emit(Event::Cancelled),
        Err(Stop::Failed(failure)) => emit(Event::Failed { failure }),
    }
}

impl Runner<'_> {
    fn execute(&self) -> Outcome<(PathBuf, u64)> {
        let request = self.request;
        fs::create_dir_all(&request.folder).map_err(|error| self.io_failure(error))?;
        let work = tempfile::Builder::new()
            .prefix(".sotto-")
            .tempdir_in(&request.folder)
            .map_err(|error| self.io_failure(error))?;

        let local = request.local_file.as_deref().filter(|_| request.mode == Mode::Dubbed);
        let steps = match (request.mode, local.is_some()) {
            (Mode::Dubbed, true) => vec!["voice", "merge"],
            (Mode::Dubbed, false) => vec!["check", "voice", "download", "merge"],
            (Mode::Video, _) => vec!["check", "download"],
            (Mode::Voice, _) => vec!["check", "voice"],
        };
        (self.emit)(Event::Plan { steps });

        if let Some(file) = local {
            return self.dub_local_file(file, work.path());
        }

        let (meta, browser) = self.resolve_video()?;
        let title = meta
            .as_ref()
            .map(|meta| clean_title(&meta.title))
            .unwrap_or_else(|| video_id(&request.link));
        let duration = meta.as_ref().and_then(|meta| meta.duration);

        let voice = work.path().join("voice.mp3");
        if request.mode != Mode::Video {
            self.fetch_voice(work.path(), browser.as_deref(), duration)?;
        }
        if request.mode == Mode::Voice {
            return self.place(&voice, &format!("{title} (RU)"), "mp3");
        }

        let video = self.download(work.path(), browser.as_deref(), meta.as_ref())?;
        if request.mode == Mode::Video {
            return self.place(&video, &title, "mp4");
        }

        let merged = work.path().join("result.mp4");
        self.merge(&video, &voice, &merged, duration, true)?;
        self.place(&merged, &format!("{title} (RU)"), "mp4")
    }

    fn dub_local_file(&self, file: &Path, work: &Path) -> Outcome<(PathBuf, u64)> {
        let (duration, has_audio) = self.probe(file)?;
        let stem = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".into());
        (self.emit)(Event::Info { title: stem.clone(), duration, height: None, thumbnail: None });

        self.fetch_voice(work, self.request.browser.as_deref(), duration)?;

        let extension = match file.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ["mp4", "m4v", "mov"].contains(&ext.to_lowercase().as_str()) => "mp4",
            _ => "mkv",
        };
        let merged = work.join(format!("result.{extension}"));
        self.merge(file, &work.join("voice.mp3"), &merged, duration, has_audio)?;
        self.place(&merged, &format!("{stem} (RU)"), extension)
    }

    // Пробует сначала запомнившийся браузер, затем без входа, затем остальные браузеры
    fn resolve_video(&self) -> Outcome<(Option<Metadata>, Option<String>)> {
        self.stage("check", "Проверяю ссылку", None);

        let mut attempts: Vec<Option<String>> = Vec::new();
        if let Some(browser) = &self.request.browser {
            attempts.push(Some(browser.clone()));
        }
        attempts.push(None);
        for browser in browsers() {
            if self.request.browser.as_deref() != Some(browser) {
                attempts.push(Some(browser.to_string()));
            }
        }

        let mut last = errors::sign_in_needed();
        for browser in attempts {
            if let Some(name) = &browser {
                self.stage("check", "Пробую вход из браузера", Some(browser_title(name).to_string()));
            }
            match self.read_metadata(browser.as_deref()) {
                Ok(meta) => {
                    if ["is_live", "is_upcoming", "post_live"].contains(&meta.live_status.as_str()) {
                        return Err(Stop::Failed(errors::live_stream()));
                    }
                    (self.emit)(Event::Browser { name: browser.clone() });
                    (self.emit)(Event::Info {
                        title: meta.title.clone(),
                        duration: meta.duration,
                        height: meta.height,
                        thumbnail: meta.thumbnail.clone(),
                    });
                    return Ok((Some(meta), browser));
                }
                Err(Stop::Failed(failure)) if matches!(failure.kind, Kind::Auth | Kind::Cookies) => {
                    if failure.kind == Kind::Auth {
                        last = failure;
                    }
                }
                Err(stop) => return Err(stop),
            }
        }

        // Озвучка часто доступна и тогда, когда сам YouTube закрыт
        if self.request.mode == Mode::Voice {
            return Ok((None, None));
        }
        Err(Stop::Failed(last))
    }

    fn read_metadata(&self, browser: Option<&str>) -> Outcome<Metadata> {
        let mut command = self.ytdlp(browser);
        command.args(["-f", "bv+ba/b", "-S"]).arg(sort_order(&self.request.quality));
        for field in [
            "%(title).200B",
            "%(duration)s",
            "%(height)s",
            "%(filesize,filesize_approx)s",
            "%(thumbnail)s",
            "%(live_status)s",
        ] {
            command.args(["--print", field]);
        }
        command.arg(&self.request.link);

        let mut stdout = Vec::new();
        let status = self.run_tool(command, |line, is_stdout| {
            if is_stdout {
                stdout.push(line.to_string());
            }
        })?;
        if !status || stdout.len() < 6 {
            return Err(Stop::Failed(errors::explain(&self.log.lock().unwrap())));
        }
        Ok(Metadata {
            title: stdout[0].clone(),
            duration: stdout[1].parse().ok(),
            height: stdout[2].parse().ok(),
            size: stdout[3].parse().ok(),
            thumbnail: Some(stdout[4].clone()).filter(|url| url.starts_with("http")),
            live_status: stdout[5].clone(),
        })
    }

    fn fetch_voice(&self, work: &Path, browser: Option<&str>, duration: Option<f64>) -> Outcome<()> {
        self.stage(
            "voice",
            "Яндекс готовит озвучку",
            Some("Для нового видео это может занять несколько минут".to_string()),
        );

        let voice = work.join("voice.mp3");
        let stop = Arc::new(AtomicBool::new(false));
        let total = duration.map(|seconds| seconds * VOICE_BYTES_PER_SECOND);

        let mut command = Command::new(tools::tool_path("vot"));
        command
            .env("PATH", tools::search_path())
            .arg("--no-visual")
            .arg("--json")
            .arg(format!("--outdir={}", work.display()))
            .arg("--outfile=voice");
        if let Some(browser) = browser {
            command.arg(format!("--cookies-from-browser={browser}"));
        }
        command.arg(&self.request.link);

        let mut succeeded = None;
        thread::scope(|scope| -> Outcome<()> {
            let watcher = {
                let stop = stop.clone();
                let voice = voice.clone();
                let emit = self.emit;
                scope.spawn(move || watch_voice(&voice, total, &stop, emit))
            };
            let result = self.run_tool(command, |line, is_stdout| {
                if is_stdout {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                        succeeded = value["ok"].as_bool();
                    }
                }
            });
            stop.store(true, Ordering::SeqCst);
            let _ = watcher.join();
            result.map(|_| ())
        })?;

        let has_file = fs::metadata(&voice).map(|meta| meta.len() > 0).unwrap_or(false);
        if succeeded == Some(true) && has_file {
            Ok(())
        } else {
            Err(Stop::Failed(errors::translate_refused()))
        }
    }

    fn download(
        &self,
        work: &Path,
        browser: Option<&str>,
        meta: Option<&Metadata>,
    ) -> Outcome<PathBuf> {
        self.stage("download", "Скачиваю видео", None);

        let mut command = self.ytdlp(browser);
        command
            .args(["-f", "bv+ba/b", "-S"])
            .arg(sort_order(&self.request.quality))
            .args(["--merge-output-format", "mp4", "--newline", "--progress-template"])
            .arg("download:@@dl %(progress.downloaded_bytes)s %(progress.total_bytes,progress.total_bytes_estimate)s %(progress.speed)s")
            .arg("-o")
            .arg(work.join("video.%(ext)s"))
            .arg(&self.request.link);

        let mut progress = DownloadProgress::new(meta.and_then(|meta| meta.size));
        let ok = self.run_tool(command, |line, _| {
            if let Some(event) = progress.read(line, self) {
                (self.emit)(event);
            }
        })?;

        let video = work.join("video.mp4");
        if ok && video.is_file() {
            Ok(video)
        } else {
            Err(Stop::Failed(errors::explain(&self.log.lock().unwrap())))
        }
    }

    fn merge(
        &self,
        video: &Path,
        voice: &Path,
        out: &Path,
        duration: Option<f64>,
        has_audio: bool,
    ) -> Outcome<()> {
        self.stage("merge", "Склеиваю видео и голос", None);

        let mut command = Command::new(tools::tool_path("ffmpeg"));
        command
            .args(["-hide_banner", "-loglevel", "warning", "-nostats", "-progress", "pipe:1", "-y", "-i"])
            .arg(video)
            .arg("-i")
            .arg(voice);
        if has_audio {
            command
                .arg("-filter_complex")
                .arg(format!(
                    "[0:a]volume={ORIGINAL_VOLUME}[orig];[orig][1:a]amix=inputs=2:duration=first:normalize=0[mix]"
                ))
                .args(["-map", "0:v:0", "-map", "[mix]"]);
        } else {
            command.args(["-map", "0:v:0", "-map", "1:a:0", "-shortest"]);
        }
        command.args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]);
        if out.extension().is_some_and(|ext| ext == "mp4") {
            command.args(["-movflags", "+faststart"]);
        }
        command.arg(out);

        let mut speed = None;
        let ok = self.run_tool(command, |line, is_stdout| {
            if !is_stdout {
                return;
            }
            if let Some(value) = line.strip_prefix("speed=") {
                speed = value.trim().trim_end_matches('x').parse::<f64>().ok();
            } else if let Some(value) = line.strip_prefix("out_time_us=") {
                if let (Ok(micros), Some(duration)) = (value.parse::<f64>(), duration) {
                    let seconds = micros / 1_000_000.0;
                    (self.emit)(Event::Progress {
                        fraction: Some((seconds / duration).min(0.995)),
                        remaining: speed.filter(|speed| *speed > 0.0).map(|speed| (duration - seconds).max(0.0) / speed),
                        speed: None,
                        done: None,
                        total: None,
                    });
                }
            }
        })?;
        if ok && out.is_file() {
            Ok(())
        } else {
            Err(Stop::Failed(errors::explain(&self.log.lock().unwrap())))
        }
    }

    // ffmpeg без выходного файла печатает сведения о входном и завершается с ошибкой, это нормально
    fn probe(&self, file: &Path) -> Outcome<(Option<f64>, bool)> {
        let mut command = Command::new(tools::tool_path("ffmpeg"));
        command.arg("-hide_banner").arg("-i").arg(file);
        let mut duration = None;
        let mut has_audio = false;
        self.run_tool(command, |line, _| {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Duration: ") {
                duration = parse_clock(rest.split(',').next().unwrap_or(""));
            }
            has_audio |= line.starts_with("Stream #") && line.contains("Audio:");
        })?;
        Ok((duration, has_audio))
    }

    fn place(&self, file: &Path, name: &str, extension: &str) -> Outcome<(PathBuf, u64)> {
        let folder = &self.request.folder;
        let mut target = folder.join(format!("{name}.{extension}"));
        let mut number = 2;
        while target.exists() {
            target = folder.join(format!("{name} {number}.{extension}"));
            number += 1;
        }
        fs::rename(file, &target).map_err(|error| self.io_failure(error))?;
        let size = fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
        Ok((target, size))
    }

    fn ytdlp(&self, browser: Option<&str>) -> Command {
        let mut command = Command::new(tools::tool_path("yt-dlp"));
        command
            .env("PATH", tools::search_path())
            .env("PYTHONUTF8", "1")
            .args(["--no-playlist", "--encoding", "utf-8", "--ffmpeg-location"])
            .arg(tools::tools_dir());
        if let Some(browser) = browser {
            command.args(["--cookies-from-browser", browser]);
        }
        command
    }

    fn stage(&self, step: &'static str, title: &str, detail: Option<String>) {
        (self.emit)(Event::Stage { step, title: title.to_string(), detail });
    }

    fn io_failure(&self, error: std::io::Error) -> Stop {
        Stop::Failed(errors::explain(&[format!("ERROR: {error}")]))
    }

    fn remember(&self, line: &str) {
        self.log.lock().unwrap().push(line.to_string());
        (self.emit)(Event::Log { line: line.to_string() });
    }

    // Запускает программу и отдаёт строки вывода по мере появления. Возвращает, успешен ли код выхода
    fn run_tool(
        &self,
        mut command: Command,
        mut on_line: impl FnMut(&str, bool),
    ) -> Outcome<bool> {
        if self.control.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        prepare(&mut command);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child: Child = command.spawn().map_err(|error| self.io_failure(error))?;
        *self.control.running.lock().unwrap() = Some(child.id());

        let (sender, receiver) = mpsc::channel();
        let stdout = child.stdout.take().map(|pipe| spawn_reader(pipe, true, sender.clone()));
        let stderr = child.stderr.take().map(|pipe| spawn_reader(pipe, false, sender));

        for (line, is_stdout) in receiver {
            // Служебный прогресс и ответы для разбора в журнал не идут
            let noisy = line.starts_with("@@")
                || (is_stdout && line.contains('=') && !line.contains(' '));
            if !noisy {
                self.remember(&line);
            }
            on_line(&line, is_stdout);
        }

        let status = child.wait();
        *self.control.running.lock().unwrap() = None;
        for reader in [stdout, stderr].into_iter().flatten() {
            let _ = reader.join();
        }

        if self.control.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        Ok(status.map(|status| status.success()).unwrap_or(false))
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    pipe: R,
    is_stdout: bool,
    sender: mpsc::Sender<(String, bool)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut buffer = Vec::new();
        // Программы обновляют строку через \r, поэтому читаем не по \n
        loop {
            let mut byte = [0u8];
            match reader.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) if byte[0] == b'\n' || byte[0] == b'\r' => {
                    if !buffer.is_empty() {
                        let line = String::from_utf8_lossy(&buffer).into_owned();
                        let _ = sender.send((line, is_stdout));
                        buffer.clear();
                    }
                }
                Ok(_) => buffer.push(byte[0]),
            }
        }
        if !buffer.is_empty() {
            let _ = sender.send((String::from_utf8_lossy(&buffer).into_owned(), is_stdout));
        }
    })
}

fn watch_voice(voice: &Path, total: Option<f64>, stop: &AtomicBool, emit: &(dyn Fn(Event) + Sync)) {
    let mut last = 0.0;
    let mut speed: Option<f64> = None;
    let mut started = false;
    let mut last_time = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
        let size = fs::metadata(voice).map(|meta| meta.len() as f64).unwrap_or(0.0);
        if size <= 0.0 {
            continue;
        }
        if !started {
            started = true;
            emit(Event::Stage { step: "voice", title: "Скачиваю озвучку".to_string(), detail: None });
        }
        let elapsed = last_time.elapsed().as_secs_f64();
        last_time = Instant::now();
        let rate = (size - last) / elapsed.max(0.1);
        last = size;
        if rate > 0.0 {
            speed = Some(speed.map_or(rate, |speed| speed * 0.8 + rate * 0.2));
        }
        emit(Event::Progress {
            fraction: total.map(|total| (size / total).min(0.995)),
            remaining: total.zip(speed).map(|(total, speed)| (total - size).max(0.0) / speed),
            speed,
            done: Some(size),
            total,
        });
    }
}

struct DownloadProgress {
    whole: Option<f64>,
    finished: f64,
    stream_total: f64,
    streams: u32,
    speed: Option<f64>,
    last_tick: Instant,
}

impl DownloadProgress {
    fn new(whole: Option<f64>) -> Self {
        Self {
            whole,
            finished: 0.0,
            stream_total: 0.0,
            streams: 0,
            speed: None,
            last_tick: Instant::now() - Duration::from_secs(1),
        }
    }

    fn read(&mut self, line: &str, runner: &Runner) -> Option<Event> {
        if line.starts_with("[download] Destination:") {
            self.finished += self.stream_total;
            self.stream_total = 0.0;
            self.streams += 1;
            let title = if self.streams == 1 { "Скачиваю видео" } else { "Скачиваю звук" };
            runner.stage("download", title, None);
            return None;
        }
        if line.starts_with("[Merger]") {
            runner.stage("download", "Собираю файл", None);
            return None;
        }
        let fields: Vec<&str> = line.strip_prefix("@@dl ")?.split(' ').collect();
        let done: f64 = fields.first()?.parse().ok()?;
        if let Some(total) = fields.get(1).and_then(|value| value.parse().ok()) {
            self.stream_total = total;
        }
        if self.last_tick.elapsed() < Duration::from_millis(250) {
            return None;
        }
        self.last_tick = Instant::now();

        let current = self.finished + done;
        let whole = self.whole.unwrap_or(0.0).max(self.finished + self.stream_total);
        if whole <= 0.0 {
            return None;
        }
        if let Some(rate) = fields.get(2).and_then(|value| value.parse::<f64>().ok()) {
            self.speed = Some(self.speed.map_or(rate, |speed| speed * 0.85 + rate * 0.15));
        }
        Some(Event::Progress {
            fraction: Some((current / whole).min(0.995)),
            remaining: self.speed.filter(|speed| *speed > 0.0).map(|speed| (whole - current).max(0.0) / speed),
            speed: self.speed,
            done: Some(current),
            total: Some(whole),
        })
    }
}

fn sort_order(quality: &str) -> String {
    // h264 в приоритете: vp9 и av1 в mp4 не открываются в QuickTime
    if quality == "max" {
        "res,vcodec:h264,acodec:m4a".to_string()
    } else {
        format!("res:{quality},vcodec:h264,acodec:m4a")
    }
}

fn browsers() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["firefox", "chrome", "safari", "brave", "edge"]
    } else if cfg!(windows) {
        // Chrome на Windows шифрует cookies так, что yt-dlp их часто не читает
        &["firefox", "edge", "chrome", "brave"]
    } else {
        &["firefox", "chrome", "chromium", "brave", "edge"]
    }
}

fn browser_title(name: &str) -> &'static str {
    match name {
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "chromium" => "Chromium",
        "safari" => "Safari",
        "brave" => "Brave",
        "edge" => "Edge",
        _ => "браузер",
    }
}

pub fn clean_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|symbol| match symbol {
            '/' | '\\' | '|' => '-',
            ':' => '-',
            '<' | '>' | '"' | '?' | '*' => ' ',
            symbol if symbol.is_control() => ' ',
            symbol => symbol,
        })
        .collect();
    let name = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.trim_matches(|symbol| symbol == '.' || symbol == ' ');
    if name.is_empty() { "video".to_string() } else { name.to_string() }
}

pub fn video_id(link: &str) -> String {
    let path = link.split_once("://").map_or(link, |(_, rest)| rest);
    let (location, query) = path.split_once('?').unwrap_or((path, ""));
    let params: HashMap<&str, &str> = query.split('&').filter_map(|pair| pair.split_once('=')).collect();
    if let Some(id) = params.get("v") {
        return clean_title(id);
    }
    clean_title(location.rsplit('/').next().unwrap_or("video"))
}

fn parse_clock(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    for part in text.trim().split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_safe_on_every_system() {
        assert_eq!(clean_title("Intro: A/B? \"test\"."), "Intro- A-B test");
        assert_eq!(clean_title("..."), "video");
    }

    #[test]
    fn finds_video_id() {
        assert_eq!(video_id("https://www.youtube.com/watch?v=abc123&list=x&t=7s"), "abc123");
        assert_eq!(video_id("https://youtu.be/abc123?t=7"), "abc123");
    }

    #[test]
    fn parses_ffmpeg_duration() {
        assert_eq!(parse_clock("01:02:03.50"), Some(3723.5));
        assert_eq!(parse_clock("N/A"), None);
    }

    #[test]
    fn builds_sort_order() {
        assert_eq!(sort_order("720"), "res:720,vcodec:h264,acodec:m4a");
        assert_eq!(sort_order("max"), "res,vcodec:h264,acodec:m4a");
    }
}
