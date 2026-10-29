use super::PageRequest;
use crate::error::ApiResult;
use crate::inertia::props::*;

pub(super) async fn home(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let home = request
        .when("home", async {
            Ok(request.services.collection.home(request.actor).await?)
        })
        .await?;
    Ok(PageProps::Home(HomePageProps {
        shared: request.shared,
        home,
    }))
}
