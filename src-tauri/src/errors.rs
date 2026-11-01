use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    // Нужен вход в YouTube: имеет смысл попробовать другой браузер
    Auth,
    // Не удалось прочитать cookies именно этого браузера
    Cookies,
    Translate,
    Network,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub kind: Kind,
    pub title: String,
    pub message: String,
    pub hint: Option<String>,
}

fn failure(kind: Kind, title: &str, message: &str, hint: Option<&str>) -> Failure {
    Failure {
        kind,
        title: title.into(),
        message: message.into(),
        hint: hint.map(Into::into),
    }
}

pub fn translate_refused() -> Failure {
    failure(
        Kind::Translate,
        "Яндекс не озвучил это видео",
        "Перевод недоступен: видео могло оказаться слишком длинным, без речи или не на английском.",
        Some("Можно скачать его без озвучки."),
    )
}

pub fn live_stream() -> Failure {
    failure(
        Kind::Other,
        "Это прямой эфир",
        "Скачать можно только готовую запись.",
        Some("Попробуйте снова, когда эфир закончится."),
    )
}

pub fn sign_in_needed() -> Failure {
    failure(
        Kind::Auth,
        "YouTube просит войти",
        "Без входа в аккаунт видео не отдаётся, а взять вход из браузера не получилось.",
        Some("Войдите в YouTube в браузере и разрешите доступ, когда система спросит. Или получите только озвучку либо укажите свой файл."),
    )
}

pub fn setup_failed(detail: &str) -> Failure {
    failure(
        Kind::Network,
        "Не удалось скачать компоненты",
        detail,
        Some("Проверьте интернет и VPN, затем повторите."),
    )
}

// Строки вывода yt-dlp и ffmpeg, из которых видно причину сбоя
fn error_lines(log: &[String]) -> Vec<&str> {
    log.iter()
        .rev()
        .take(60)
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("error") || lower.contains("failed") || lower.contains("cannot")
        })
        .map(String::as_str)
        .collect()
}

pub fn explain(log: &[String]) -> Failure {
    let errors = error_lines(log);
    let text = errors.join("\n").to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));

    if has(&["no space left", "not enough space"]) {
        return failure(
            Kind::Other,
            "На диске закончилось место",
            "Файл не поместился на диск.",
            Some("Освободите место или выберите качество ниже."),
        );
    }
    if has(&["sign in to confirm", "not a bot"]) {
        return sign_in_needed();
    }
    if has(&[
        "could not find",
        "failed to decrypt",
        "keychain",
        "cookies database",
        "could not copy",
        "permission denied",
        "operation not permitted",
    ]) && text.contains("cookie")
    {
        return failure(
            Kind::Cookies,
            "Не удалось прочитать браузер",
            "Sotto не смог взять вход в YouTube из браузера.",
            Some("Когда система спросит доступ, разрешите его. Без этого видео может не скачаться."),
        );
    }
    if has(&["private video"]) {
        return failure(
            Kind::Other,
            "Это закрытое видео",
            "Автор ограничил доступ к нему.",
            Some("Если доступ у вас есть, войдите в этот аккаунт в браузере."),
        );
    }
    if has(&["members-only", "join this channel"]) {
        return failure(
            Kind::Other,
            "Видео только для спонсоров канала",
            "Для него нужна платная подписка на канал.",
            None,
        );
    }
    if has(&["confirm your age", "age-restricted", "inappropriate for some users"]) {
        return failure(
            Kind::Auth,
            "Видео с ограничением по возрасту",
            "YouTube требует подтвердить возраст.",
            Some("Войдите в браузере в аккаунт, где возраст подтверждён."),
        );
    }
    if has(&["not available in your country", "geo restriction", "geo-restricted"]) {
        return failure(
            Kind::Other,
            "Видео недоступно в этой стране",
            "Автор ограничил показ по странам.",
            Some("Попробуйте другой сервер VPN."),
        );
    }
    if has(&["video unavailable", "video is unavailable", "has been removed", "account associated"]) {
        return failure(
            Kind::Other,
            "Видео недоступно",
            "Его удалили или ссылка неверная.",
            Some("Проверьте, открывается ли видео в браузере."),
        );
    }
    if has(&["unsupported url", "incomplete youtube id", "invalid url", "is not a valid url"]) {
        return failure(
            Kind::Other,
            "Ссылка не подошла",
            "По этой ссылке нет видео.",
            Some("Скопируйте адрес со страницы самого видео."),
        );
    }
    if has(&["429", "too many requests"]) {
        return failure(
            Kind::Other,
            "YouTube ограничил запросы",
            "Слишком много загрузок за короткое время.",
            Some("Подождите минут десять и повторите."),
        );
    }
    if has(&[
        "timed out",
        "network is unreachable",
        "connection",
        "name or service not known",
        "nodename nor servname",
        "getaddrinfo failed",
        "handshake",
        "urlopen",
    ]) {
        return failure(
            Kind::Network,
            "Нет связи с YouTube",
            "Соединение оборвалось или не установилось.",
            Some("Проверьте интернет и VPN, затем повторите."),
        );
    }
    let last = errors
        .first()
        .copied()
        .or(log.last().map(String::as_str))
        .unwrap_or("Загрузка завершилась с ошибкой.");
    failure(
        Kind::Other,
        "Что-то пошло не так",
        last,
        Some("Журнал поможет понять причину."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(line: &str) -> (Kind, String) {
        let failure = explain(&[line.to_string()]);
        (failure.kind, failure.title)
    }

    #[test]
    fn recognises_bot_check() {
        let line = "ERROR: [youtube] abc: Sign in to confirm you're not a bot. Use --cookies-from-browser";
        assert_eq!(kind_of(line).0, Kind::Auth);
    }

    #[test]
    fn recognises_missing_browser() {
        let line = "ERROR: could not find firefox cookies database in /Users/x";
        assert_eq!(kind_of(line).0, Kind::Cookies);
    }

    #[test]
    fn recognises_unavailable_video() {
        assert_eq!(kind_of("ERROR: [youtube] abc: Video unavailable").1, "Видео недоступно");
    }

    #[test]
    fn recognises_dead_network() {
        assert_eq!(kind_of("ERROR: Unable to download: timed out").0, Kind::Network);
    }

    #[test]
    fn falls_back_to_raw_message() {
        let failure = explain(&["ERROR: something new".to_string()]);
        assert_eq!(failure.message, "ERROR: something new");
    }
}
