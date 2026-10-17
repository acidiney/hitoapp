use std::sync::Arc;

use axum::{Extension, Json};
use utoipa::{
    OpenApi,
    openapi::{
        self,
        security::{Http, HttpAuthScheme, SecurityScheme},
    },
};

#[derive(OpenApi)]
#[openapi(info(title = "Hito Community API", version = "1.0.0"))]
struct Contract;

pub(crate) fn base_document() -> openapi::OpenApi {
    let mut document = Contract::openapi();
    let components = document.components.get_or_insert_with(Default::default);
    for name in ["administratorBearer", "reporterBearer"] {
        components.add_security_scheme(
            name,
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
    document
}

#[utoipa::path(get, path = "/v1/openapi.json", responses(
    (status = 200, body = Object, content_type = "application/json"),
))]
pub(crate) async fn serve(
    Extension(document): Extension<Arc<openapi::OpenApi>>,
) -> Json<openapi::OpenApi> {
    Json((*document).clone())
}
