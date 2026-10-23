use super::PageRequest;
use crate::error::ApiResult;
use crate::inertia::props::*;
use hito_core::{AudiobookLibraryQuery, Error};

pub(super) async fn index(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let collection = &request.services.collection;
    let page = if request.requested("books") || request.requested("book_pagination") {
        Some(
            collection
                .audiobook_library(
                    request.actor,
                    AudiobookLibraryQuery {
                        limit: request
                            .search
                            .limit
                            .map(u16::try_from)
                            .transpose()
                            .map_err(|_| {
                                Error::Invalid("Audiobook pages contain 1 to 100 books".into())
                            })?,
                        cursor: request.pagination.cursor.clone(),
                    },
                )
                .await?,
        )
    } else {
        None
    };
    let (books, book_pagination) = match page {
        Some(page) => (
            request.requested("books").then_some(page.items),
            request
                .requested("book_pagination")
                .then_some(page.pagination),
        ),
        None => (None, None),
    };
    let grouping_playlists = request
        .when("grouping_playlists", async {
            Ok(collection
                .audiobook_grouping_playlists(request.actor)
                .await?)
        })
        .await?;
    Ok(PageProps::AudiobookIndex(AudiobookIndexPageProps {
        shared: request.shared,
        books,
        book_pagination,
        grouping_playlists,
    }))
}

pub(super) async fn detail(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .services
        .collection
        .audiobook(request.actor, id)
        .await?;
    Ok(PageProps::Audiobook(AudiobookPageProps {
        detail: request.requested("detail").then_some(detail),
        shared: request.shared,
    }))
}
