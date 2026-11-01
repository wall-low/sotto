// Запуск ядра без окна, для проверки из терминала:
//   cargo run --example cli -- install
//   cargo run --example cli -- run dubbed 720 ./out 'https://youtu.be/...' [browser] [local-file] [cancel-after]
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sotto_lib::job::{self, Control, Event, Mode, Request};
use sotto_lib::tools;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("install") => {
            let last = std::cell::RefCell::new(String::new());
            let result = tools::install(|progress| {
                let line = format!("{} {}/{}", progress.title, progress.index, progress.count);
                if *last.borrow() != line {
                    println!("{line}");
                    last.replace(line);
                }
            });
            println!("{result:?}");
        }
        Some("run") => run(&args[1..]),
        _ => println!("{}", serde_json::to_string(&tools::status()).unwrap()),
    }
}

fn run(args: &[String]) {
    let mode = match args[0].as_str() {
        "video" => Mode::Video,
        "voice" => Mode::Voice,
        _ => Mode::Dubbed,
    };
    let optional = |index: usize| args.get(index).filter(|value| !value.is_empty() && *value != "-");
    let request = Request {
        link: args[3].clone(),
        mode,
        quality: args[1].clone(),
        folder: PathBuf::from(&args[2]),
        browser: optional(4).cloned(),
        local_file: optional(5).map(PathBuf::from),
    };
    let control = Arc::new(Control::default());
    if let Some(seconds) = optional(6).and_then(|value| value.parse().ok()) {
        let control = control.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs_f64(seconds));
            control.cancel();
        });
    }

    let started = Instant::now();
    let shown = std::sync::Mutex::new(Instant::now() - Duration::from_secs(9));
    job::run(&request, &control, &|event: Event| {
        if let Event::Progress { .. } = event {
            let mut shown = shown.lock().unwrap();
            if shown.elapsed() < Duration::from_secs(8) {
                return;
            }
            *shown = Instant::now();
        }
        if !matches!(event, Event::Log { .. }) {
            println!("{:6.1} {}", started.elapsed().as_secs_f64(), serde_json::to_string(&event).unwrap());
        }
    });
}
