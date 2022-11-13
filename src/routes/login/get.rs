
use actix_web::{get, HttpResponse};
use actix_web::http::header::ContentType;

#[get("/login")]
pub async fn login_form() -> HttpResponse {
    return HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(include_str!("login.html"));
}
