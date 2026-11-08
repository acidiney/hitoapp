pub(crate) const FALLBACK_POLICY: &str = "hito.provider-rank-fallback.v1";

#[derive(Clone, Copy)]
pub(super) enum Basis {
    Matched,
    ProviderRank { index: usize },
}

impl Basis {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::ProviderRank { .. } => "provider_rank",
        }
    }

    pub(super) fn rank_index(self) -> Option<usize> {
        match self {
            Self::Matched => None,
            Self::ProviderRank { index } => Some(index),
        }
    }
}
