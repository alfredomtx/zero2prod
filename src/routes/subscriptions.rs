use actix_web::{post, web, HttpResponse};
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{NewSubscriber, SubscriberName, SubscriberEmail};
use crate::email_client::EmailClient;

#[derive(Debug)]
#[derive(serde::Deserialize)]
pub struct FormData {
    email: String,
    name: String,
}

pub fn parse_subscriber(form: FormData) -> Result<NewSubscriber, String> {
    let name = SubscriberName::parse(form.name)?;
    let email = SubscriberEmail::parse(form.email)?;
    Ok(NewSubscriber{ email, name })
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(
    name = "Adding a new subscribe",
    skip(form, pool),
    fields(
        subscriber_email = %form.email,
        subscriber_name = %form.name
    )
)]
#[post("/subscriptions")]
pub async fn subscribe(
    form: web::Form<FormData>,
    pool: web::Data<PgPool>,
    // Get the email client from the app context
    email_client: web::Data<EmailClient>,
) -> HttpResponse {
    // `web::Form` is a wrapper around `FormData`
    // `form.0` gives us access to the underlying `FormData`
    let new_subscriber = match parse_subscriber(form.0) {
        Ok(subscriber) => subscriber,
        Err(_) => return HttpResponse::BadRequest().finish()
    };

    if insert_subscriber(&pool, &new_subscriber).await.is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    let confirmation_link = "https://my-api.com/subscriptions/confirm";

    // Send a (useless) email to the new subscriber.
    // We are ignoring email delivery errors for now.
    if email_client.send_email(
        new_subscriber.email,
        "Welcome!",
        &format!("Welcome to our newsletter!<br />\
        Click <a href=\"{}\">here</a> to confirm your subscription.", confirmation_link),
        &format!("Welcome to our newsletter!\nVisit {} to confirm your subscription.",
        confirmation_link)
    )
    .await
    .is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().finish()
}


#[tracing::instrument(
    name = "Saving new subscriber details in the database"
    skip(new_subscriber, pool)
)]
pub async fn insert_subscriber(pool: &PgPool, new_subscriber: &NewSubscriber) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
            INSERT INTO subscriptions (id, email, name, subscribed_at, status)
            VALUES ($1, $2, $3, $4, 'confirmed')
        "#,
        Uuid::new_v4(),
        new_subscriber.email.as_ref(),
        new_subscriber.name.as_ref(),
        Utc::now()
    )
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to execute query: {:?}", e);
        e
    // Using the `?` operator to return early
    // if the function failed, returning a sqlx::Error
    // We will talk about error handling in depth later!
    })?;
    Ok(())
}