const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const stored = (key) => localStorage.getItem(key);

const state = {
  mode: stored("mode") || "dubbed",
  quality: stored("quality") || "1080",
  folder: stored("folder"),
  localFile: null,
  steps: [],
  step: null,
  result: null,
  log: [],
};

function show(screen) {
  for (const section of document.querySelectorAll(".screen")) {
    section.hidden = section.id !== screen;
  }
}

function fileName(path) {
  return path.split(/[\\/]/).pop();
}

function duration(seconds) {
  const total = Math.max(Math.round(seconds), 1);
  if (total < 60) return `${total} с`;
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  return hours > 0 ? `${hours} ч ${minutes} мин` : `${minutes} мин ${total % 60} с`;
}

function bytes(value) {
  const megabytes = value / 1e6;
  if (megabytes < 1000) return `${Math.round(megabytes)} МБ`;
  return `${(megabytes / 1000).toFixed(1).replace(".", ",")} ГБ`;
}

function clock(seconds) {
  const total = Math.max(Math.floor(seconds), 0);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

// Форма

function linkProblem(text) {
  if (!text) return null;
  let url;
  try {
    url = new URL(text);
  } catch {
    return "Это не похоже на ссылку на YouTube";
  }
  const host = url.hostname.toLowerCase();
  if (host !== "youtu.be" && host !== "youtube.com" && !host.endsWith(".youtube.com")) {
    return "Это не похоже на ссылку на YouTube";
  }
  if (url.pathname === "/playlist") return "Это плейлист, нужна ссылка на одно видео";
  return null;
}

function refreshForm() {
  const link = $("link").value.trim();
  const problem = linkProblem(link);
  $("link-problem").textContent = problem || "";
  $("start").disabled = !link || Boolean(problem);

  const modes = [...document.querySelectorAll("#modes button")];
  modes.forEach((button, index) => {
    const active = button.dataset.mode === state.mode;
    button.classList.toggle("active", active);
    if (active) $("modes").style.setProperty("--index", index);
  });

  const usesFile = state.mode === "dubbed" && state.localFile;
  $("file-row").hidden = state.mode !== "dubbed";
  $("quality-row").hidden = state.mode === "voice" || Boolean(usesFile);
  $("quality").value = state.quality;
  $("file").textContent = usesFile ? fileName(state.localFile) : "Выбрать";
  $("file-clear").hidden = !usesFile;
  $("folder").textContent = state.folder ? fileName(state.folder) : "";

  const notes = {
    voice: "Получится mp3 с русской озвучкой. Обычно работает, даже если YouTube закрыт.",
    file: "Озвучка возьмётся по ссылке, видео из вашего файла.",
    max: "До 4K. Такой файл может не открыться в QuickTime.",
  };
  const note = state.mode === "voice" ? notes.voice
    : usesFile ? notes.file
    : state.quality === "max" ? notes.max
    : state.notice || "";
  $("form-note").textContent = note;

  $("start").textContent = {
    dubbed: "Скачать с озвучкой",
    video: "Скачать видео",
    voice: "Скачать озвучку",
  }[state.mode];
}

function start(overrides = {}) {
  const request = {
    link: $("link").value.trim(),
    mode: overrides.mode || state.mode,
    quality: state.quality,
    folder: state.folder,
    localFile: overrides.localFile === undefined ? state.localFile : overrides.localFile,
    browser: stored("browser"),
  };
  state.notice = null;
  state.log = [];
  state.lastRequest = request;
  resetRun();
  show("run");
  invoke("start_job", { request });
}

function resetRun() {
  $("thumb").hidden = true;
  $("thumb").removeAttribute("src");
  $("run-title").textContent = "Узнаю, что за видео";
  $("run-meta").textContent = "";
  setStage("Запускаю", null);
  setProgress({});
}

// Ход работы

let stageStarted = Date.now();
let stageDetail = null;
let hasEstimate = false;

function setStage(title, detail) {
  $("stage").textContent = title;
  stageDetail = detail;
  stageStarted = Date.now();
  setProgress({});
}

function setProgress({ fraction, remaining, speed, done, total }) {
  const known = typeof fraction === "number";
  $("bar").classList.toggle("waiting", !known);
  $("bar-fill").style.setProperty("--value", known ? fraction : 0);

  const index = state.steps.indexOf(state.step) + 1;
  $("percent").textContent = known ? `${Math.floor(fraction * 100)}%`
    : index > 0 ? `Шаг ${index} из ${state.steps.length}` : "";

  hasEstimate = typeof remaining === "number";
  if (hasEstimate) $("stat-left").textContent = `Осталось около ${duration(remaining)}`;
  else tick();

  const right = [];
  if (done && total) right.push(`${bytes(done)} из ${bytes(total)}`);
  if (speed) right.push(`${bytes(speed)}/с`);
  $("stat-right").textContent = right.join(" · ") || stageDetail || "";
}

function tick() {
  if (!hasEstimate) $("stat-left").textContent = `Прошло ${clock((Date.now() - stageStarted) / 1000)}`;
}

setInterval(tick, 1000);

function onJobEvent({ payload: event }) {
  switch (event.type) {
    case "plan":
      state.steps = event.steps;
      break;
    case "stage":
      state.step = event.step;
      setStage(event.title, event.detail);
      break;
    case "info": {
      $("run-title").textContent = event.title;
      const parts = [];
      if (event.duration) parts.push(duration(event.duration));
      if (event.height) parts.push(`${event.height}p`);
      $("run-meta").textContent = parts.join(" · ") || "";
      if (event.thumbnail) {
        $("thumb").src = event.thumbnail;
        $("thumb").hidden = false;
      }
      break;
    }
    case "progress":
      setProgress(event);
      break;
    case "browser":
      if (event.name) localStorage.setItem("browser", event.name);
      else localStorage.removeItem("browser");
      break;
    case "log":
      state.log.push(event.line);
      if (state.log.length > 500) state.log.shift();
      break;
    case "finished":
      state.result = event.path;
      $("done-name").textContent = fileName(event.path);
      $("done-meta").textContent = `${bytes(event.size)} · ${fileName(state.folder)}`;
      show("done");
      break;
    case "failed":
      showFailure(event.failure);
      break;
    case "cancelled":
      state.notice = "Загрузка отменена";
      refreshForm();
      show("form");
      break;
  }
}

function showFailure(failure) {
  $("fail-title").textContent = failure.title;
  $("fail-message").textContent = failure.message;
  $("fail-hint").textContent = failure.hint || "";

  const primary = $("fail-primary");
  const secondary = $("fail-secondary");
  secondary.hidden = true;

  if (failure.kind === "translate") {
    primary.textContent = "Скачать без озвучки";
    primary.onclick = () => start({ mode: "video", localFile: null });
  } else if (failure.kind === "auth") {
    primary.textContent = "Скачать только озвучку";
    primary.onclick = () => start({ mode: "voice", localFile: null });
    secondary.hidden = false;
    secondary.textContent = "Указать своё видео";
    secondary.onclick = async () => {
      const file = await invoke("pick_video");
      if (file) start({ mode: "dubbed", localFile: file });
    };
  } else {
    primary.textContent = "Повторить";
    primary.onclick = () => start(state.lastRequest);
  }
  show("fail");
}

// Первый запуск

async function installTools() {
  $("setup-error").hidden = true;
  $("setup-button").disabled = true;
  $("setup-progress").hidden = false;
  await invoke("install_tools");
}

async function openForm() {
  if (!state.folder) state.folder = await invoke("default_folder");
  refreshForm();
  show("form");
  $("link").focus();
}

listen("job", onJobEvent);

listen("setup-progress", ({ payload }) => {
  const overall = (payload.index - 1 + payload.fraction) / payload.count;
  $("setup-stage").textContent = `Скачиваю: ${payload.title}`;
  $("setup-percent").textContent = `${Math.floor(overall * 100)}%`;
  $("setup-bar").style.setProperty("--value", overall);
});

listen("setup-done", openForm);

listen("setup-failed", ({ payload }) => {
  $("setup-error").textContent = `${payload.title}. ${payload.hint}`;
  $("setup-error").hidden = false;
  $("setup-progress").hidden = true;
  $("setup-button").disabled = false;
  $("setup-button").textContent = "Повторить";
});

// Кнопки

$("setup-button").onclick = installTools;
$("link").oninput = refreshForm;
$("link").onkeydown = (event) => {
  if (event.key === "Enter" && !$("start").disabled) start();
};
$("start").onclick = () => start();

for (const button of document.querySelectorAll("#modes button")) {
  button.onclick = () => {
    state.mode = button.dataset.mode;
    localStorage.setItem("mode", state.mode);
    refreshForm();
  };
}

$("quality").onchange = (event) => {
  state.quality = event.target.value;
  localStorage.setItem("quality", state.quality);
  refreshForm();
};

$("folder").onclick = async () => {
  const folder = await invoke("pick_folder");
  if (folder) {
    state.folder = folder;
    localStorage.setItem("folder", folder);
    refreshForm();
  }
};

$("file").onclick = async () => {
  const file = await invoke("pick_video");
  if (file) {
    state.localFile = file;
    refreshForm();
  }
};

$("file-clear").onclick = () => {
  state.localFile = null;
  refreshForm();
};

$("cancel").onclick = () => invoke("cancel_job");
$("open").onclick = () => invoke("open_file", { path: state.result });
$("reveal").onclick = () => invoke("reveal_file", { path: state.result });
$("again").onclick = () => {
  $("link").value = "";
  state.localFile = null;
  refreshForm();
  show("form");
  $("link").focus();
};
$("back").onclick = () => {
  refreshForm();
  show("form");
};

for (const button of document.querySelectorAll(".show-log")) {
  button.onclick = () => {
    $("log-text").textContent = state.log.join("\n");
    $("log").showModal();
  };
}
$("log-close").onclick = () => $("log").close();
$("log-copy").onclick = () => navigator.clipboard.writeText(state.log.join("\n"));

document.body.classList.toggle("mac", navigator.userAgent.includes("Mac"));

invoke("tools_status").then((status) => {
  if (status.ready) openForm();
  else show("setup");
});
