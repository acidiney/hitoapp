use axum::http::HeaderMap;

#[derive(Clone, Copy)]
pub(super) struct Selection {
    pub first: u64,
    pub last: u64,
}

impl Selection {
    pub fn length(self) -> u64 {
        self.last - self.first + 1
    }
    pub fn header(self) -> String {
        format!("bytes={}-{}", self.first, self.last)
    }
}

pub(super) fn read(headers: &HeaderMap, head: bool, total: u64) -> Result<Option<Selection>, ()> {
    if head || headers.contains_key("if-range") {
        return Ok(None);
    }
    let mut values = headers.get_all("range").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let value = value.to_str().map_err(|_| ())?;
    if value.len() > 1024 {
        return Err(());
    }
    let (unit, value) = value.trim().split_once('=').ok_or(())?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return Ok(None);
    }
    let (first, last) = value.split_once('-').ok_or(())?;
    if first.is_empty() {
        let length = decimal(last).ok_or(())?;
        if length == 0 || total == 0 {
            return Err(());
        }
        return Ok(Some(Selection {
            first: total.saturating_sub(length),
            last: total - 1,
        }));
    }
    let first = decimal(first).ok_or(())?;
    let last = if last.is_empty() {
        total.saturating_sub(1)
    } else {
        decimal(last).ok_or(())?.min(total.saturating_sub(1))
    };
    if first >= total || last < first {
        return Err(());
    }
    Ok(Some(Selection { first, last }))
}

pub(super) fn decimal(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}
