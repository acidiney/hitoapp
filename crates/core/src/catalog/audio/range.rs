#[derive(Clone, Copy)]
pub enum ByteRange {
    From { first: u64, last: Option<u64> },
    Suffix { length: u64 },
}
impl ByteRange {
    pub(crate) fn resolve(self, length: u64) -> Option<std::ops::RangeInclusive<u64>> {
        if length == 0 {
            return None;
        }
        match self {
            Self::From { first, last } => {
                let last = last.unwrap_or(length - 1).min(length - 1);
                (first <= last && first < length).then_some(first..=last)
            }
            Self::Suffix { length: suffix } => {
                (suffix > 0).then(|| length.saturating_sub(suffix)..=length - 1)
            }
        }
    }
    pub(crate) fn header(self) -> String {
        match self {
            Self::From {
                first,
                last: Some(last),
            } => format!("bytes={first}-{last}"),
            Self::From { first, last: None } => format!("bytes={first}-"),
            Self::Suffix { length } => format!("bytes=-{length}"),
        }
    }
    pub(crate) fn accepts(
        self,
        received: &std::ops::RangeInclusive<u64>,
        total: Option<u64>,
    ) -> bool {
        if let Some(total) = total {
            return self.resolve(total).is_some_and(|expected| {
                expected.start() == received.start() && received.end() <= expected.end()
            });
        }
        match self {
            Self::From { first, last } => {
                first == *received.start() && last.is_none_or(|last| *received.end() <= last)
            }
            Self::Suffix { .. } => false,
        }
    }
}
