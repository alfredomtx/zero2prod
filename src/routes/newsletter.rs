use actix_web::{web, post, HttpResponse};

// Dummy implementation
#[post("/newsletter")]
pub async fn publish_newsletter(_body: web::Json<BodyData>) -> HttpResponse {
    return HttpResponse::Ok().finish();
}


#[derive(serde::Deserialize)]
pub struct BodyData {
    title: String,
    content: Content
}

#[derive(serde::Deserialize)]
pub struct Content {
    html: String,
    text: String
}
