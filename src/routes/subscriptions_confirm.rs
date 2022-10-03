use actix_web::{web, get, HttpResponse};

#[derive(serde::Deserialize)]
pub struct Parameters {
    subscription_token: String,
}

#[tracing::instrument(
    name = "Confirm a pending subscriber",
    skip(_parameters)
)]
#[get("/subscriptions/confirm")]
pub async fn confirm(_parameters: web::Query<Parameters>) -> HttpResponse {
    return HttpResponse::Ok().finish();
}



