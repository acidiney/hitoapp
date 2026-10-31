use super::PageRequest;
use crate::error::ApiResult;
use crate::inertia::props::*;

pub(super) async fn genre(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let key = request.search.q.clone().ok_or(hito_core::Error::NotFound)?;
    let genre = request
        .when("genre", async {
            Ok(request.services.catalog.genre(request.actor, &key).await?)
        })
        .await?;
    Ok(PageProps::Genre(GenrePageProps {
        shared: request.shared,
        genre,
    }))
}
