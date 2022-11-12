use actix_web::{get, HttpResponse};
use actix_web::http::header::ContentType;

#[get("/")]
pub async fn home() -> HttpResponse {
    return HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(include_str!("home.html"));
}
