// Файлы и контрольные суммы компонентов для каждой системы. Версии заданы в tools.rs

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const TARGET: Target = Target {
    ytdlp: "yt-dlp_macos",
    vot: "vot-darwin-arm64.tar.gz",
    vot_pack: Pack::TarGz,
    vot_sha256: "670811740b807a4a19f0cd243f489c235fbff2655f73f796cd005d9d7d3a7625",
    ffmpeg: "ffmpeg-darwin-arm64.gz",
    ffmpeg_sha256: "8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa",
    deno: "deno-aarch64-apple-darwin.zip",
    deno_sha256: "9f0bf660393da2eaff044cdc7444cb08c73f235849b61d224d825a194ce7c98e",
};

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const TARGET: Target = Target {
    ytdlp: "yt-dlp_macos",
    vot: "vot-darwin-x64.tar.gz",
    vot_pack: Pack::TarGz,
    vot_sha256: "22b7e2de10624f451769f415c8715ac26468fc40412c97553046deb355a3c040",
    ffmpeg: "ffmpeg-darwin-x64.gz",
    ffmpeg_sha256: "929b375c1182d956c51f7ac25e0b2b0411fb01f6f407aa15c9758efeb4242106",
    deno: "deno-x86_64-apple-darwin.zip",
    deno_sha256: "4b2f5edd2639b7470f034238b321d7002d8dfb4726d8558f52c17afac370eb2c",
};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const TARGET: Target = Target {
    ytdlp: "yt-dlp_linux",
    vot: "vot-linux-x64.tar.gz",
    vot_pack: Pack::TarGz,
    vot_sha256: "f6bc5b4bf7d074372feb7ba66d41248c067ea0c9407aa89d0a5e1794ba07c9b6",
    ffmpeg: "ffmpeg-linux-x64.gz",
    ffmpeg_sha256: "bfe8a8fc511530457b528c48d77b5737527b504a3797a9bc4866aeca69c2dffa",
    deno: "deno-x86_64-unknown-linux-gnu.zip",
    deno_sha256: "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609",
};

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const TARGET: Target = Target {
    ytdlp: "yt-dlp_linux_aarch64",
    vot: "vot-linux-arm64.tar.gz",
    vot_pack: Pack::TarGz,
    vot_sha256: "0d2aef0d54f8071955659c61ee8b994dbc229dc8819b8b0189d0bafd3ed88fbe",
    ffmpeg: "ffmpeg-linux-arm64.gz",
    ffmpeg_sha256: "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0",
    deno: "deno-aarch64-unknown-linux-gnu.zip",
    deno_sha256: "d49596c20b9788cf8f5d8ddf644103d4bde56ed239753f0bdb92ba9361d51905",
};

#[cfg(all(windows, target_arch = "x86_64"))]
const TARGET: Target = Target {
    ytdlp: "yt-dlp.exe",
    vot: "vot-windows-x64.exe.zip",
    vot_pack: Pack::Zip,
    vot_sha256: "df022abffaa808bad5be487e49040e9d86207a3250ec7061ef34218ba1df309c",
    ffmpeg: "ffmpeg-win32-x64.gz",
    ffmpeg_sha256: "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
    deno: "deno-x86_64-pc-windows-msvc.zip",
    deno_sha256: "CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF",
};
