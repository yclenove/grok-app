//! Scoped imported-history image delivery. No path or MIME comes from the page.
use super::*;
use crate::session_migration::image_reader::{self, ImageScope, ImageState};

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Variant {
    Thumbnail,
    Original,
}

#[derive(serde::Deserialize)]
pub(super) struct ImageQuery {
    t: String,
    s: String,
    a: String,
    variant: Variant,
}

pub(super) async fn image_get(
    State(state): State<ServerState>,
    Query(q): Query<ImageQuery>,
    req: Request<Body>,
) -> Response {
    let acao = cors_origin_from_headers(req.headers());
    if !request_origin_allowed(req.headers()) {
        return text_status_cors(StatusCode::FORBIDDEN, "origin not allowed", acao);
    }
    if !tokens_equal(state.token.as_str(), &q.t) {
        return text_status_cors(StatusCode::UNAUTHORIZED, "unauthorized", acao);
    }
    if q.s.len() > 1024 || q.a.len() > 512 {
        return text_status_cors(StatusCode::BAD_REQUEST, "invalid image identity", acao);
    }
    let scope: ImageScope = match serde_json::from_str(&q.s) {
        Ok(scope) => scope,
        Err(_) => return text_status_cors(StatusCode::BAD_REQUEST, "invalid image scope", acao),
    };
    let head = req.method() == Method::HEAD;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = image_reader::acquire();
        let image = image_reader::read(&scope, &q.a)?;
        let (bytes, mime) = match q.variant {
            Variant::Original => (image.bytes, image.info.mime),
            Variant::Thumbnail => {
                let (bytes, _, _) =
                    crate::image_thumb::thumbnail_jpeg_from_untrusted_bytes(&image.bytes)
                        .map_err(|_| ImageState::Invalid)?;
                (bytes, "image/jpeg")
            }
        };
        Ok::<_, ImageState>((bytes, mime))
    })
    .await;
    match result {
        Ok(Ok((bytes, mime))) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            if let Some(origin) = acao {
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    HeaderValue::from_static(origin),
                );
            }
            headers.insert(header::VARY, HeaderValue::from_static("Origin"));
            (
                StatusCode::OK,
                headers,
                if head {
                    Body::empty()
                } else {
                    Body::from(bytes)
                },
            )
                .into_response()
        }
        Ok(Err(ImageState::Oversize)) => {
            text_status_cors(StatusCode::PAYLOAD_TOO_LARGE, "image too large", acao)
        }
        Ok(Err(_)) => text_status_cors(StatusCode::NOT_FOUND, "image unavailable", acao),
        Err(_) => text_status_cors(StatusCode::INTERNAL_SERVER_ERROR, "image unavailable", acao),
    }
}
