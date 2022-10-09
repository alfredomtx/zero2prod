use actix_web::{post, web, HttpResponse, ResponseError};
use actix_web::http::StatusCode;
use chrono::Utc;
use sqlx::{Postgres, PgPool, Transaction};
use uuid::Uuid;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use crate::domain::{NewSubscriber, SubscriberName, SubscriberEmail};
use crate::email_client::EmailClient;
use crate::startup::ApplicationBaseUrl;

#[derive(Debug)]
#[derive(serde::Deserialize)]
pub struct FormData {
    email: String,
    name: String,
 }


// A new error type, wrapping a sqlx::Error
#[derive(Debug)]
pub struct StoreTokenError(sqlx::Error);

impl std::fmt::Display for StoreTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "A database error was encontered while trying to store a subscription token.")
    }
}


impl std::fmt::Display for SubscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to create a new subscriber.")
    }
}

impl std::error::Error for SubscribeError {}
impl ResponseError for SubscribeError {
    fn status_code(&self) -> StatusCode {
        match self {
            SubscribeError::ValidationError(_) => StatusCode::BAD_REQUEST,
            SubscribeError::DatabaseError(_) 
            | SubscribeError::StoreTokenError(_)
            | SubscribeError::SendEmailError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug)]
pub enum SubscribeError {
    ValidationError(String),
    DatabaseError(sqlx::Error),
    StoreTokenError(StoreTokenError),
    SendEmailError(reqwest::Error),
}

impl From<reqwest::Error> for SubscribeError {
    fn from(e: reqwest::Error) -> Self {
        Self::SendEmailError(e)
    }
}

impl From<sqlx::Error> for SubscribeError {
    fn from(e: sqlx::Error) -> Self {
        Self::DatabaseError(e)
    }
}

impl From<StoreTokenError> for SubscribeError {
    fn from(e: StoreTokenError) -> Self {
        Self::StoreTokenError(e)
    }
}

impl From<String> for SubscribeError {
    fn from(e: String) -> Self {
        Self::ValidationError(e)
    }
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(
    name = "Adding a new subscribe",
    skip(form, pool, email_client, base_url),
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
    base_url: web::Data<ApplicationBaseUrl>,
) -> Result<HttpResponse, SubscribeError> {
    // `form.0` gives us access to the underlying `FormData`
    let new_subscriber = parse_subscriber(form.0).map_err(SubscribeError::ValidationError)?;
    let mut transaction = pool.begin().await?;    // `web::Form` is a wrapper around `FormData`

    let subscriber_id = insert_subscriber(&mut transaction, &new_subscriber).await?;
    let subscription_token = generate_subscription_token();
    store_token(&mut transaction, subscriber_id, &subscription_token).await?; 
    transaction.commit().await?;

    send_confirmation_email(&email_client, new_subscriber, &base_url.0, &subscription_token).await?;

    Ok(HttpResponse::Ok().finish())
}

/// Generate a random 25-characters-long case-sensitive subscription token.
fn generate_subscription_token() -> String {
    let mut rng = thread_rng();
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .map(char::from)
        .take(25)
        .collect()
}

pub fn parse_subscriber(form: FormData) -> Result<NewSubscriber, String> {
    let name = SubscriberName::parse(form.name)?;
    let email = SubscriberEmail::parse(form.email)?;
    Ok(NewSubscriber{ email, name })
}

#[tracing::instrument(
    name = "Store subscription token in the database",
    skip(subscription_token, transaction)
)]
pub async fn store_token(transaction: &mut Transaction<'_, Postgres>,subscriber_id: Uuid,
    subscription_token: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO subscription_tokens (subscription_token, subscriber_id)
        VALUES ($1, $2)"#,
        subscription_token,
        subscriber_id
    )
    .execute(transaction)
    .await
    .map_err(|e| {
        tracing::error!("Failed to execute query: {:?}", e);
        e
    })?;

    return Ok(());
}

#[tracing::instrument(
    name = "Send a confirmation email to a new subscriber",
    skip(email_client, new_subscriber, base_url)
)]
pub async fn send_confirmation_email(
    email_client: &EmailClient,
    new_subscriber: NewSubscriber,
    base_url: &str,
    subscription_token: &str
) -> Result<(), reqwest::Error> {
    let confirmation_link = format!(
        "{}/subscriptions/confirm?subscription_token={}",
        base_url, subscription_token
    );

    let plain_body = format!(
        "Welcome to our newsletter!\nVisit {} to confirm your subscription.",
            confirmation_link
        );
    let html_body = format!(
        "Welcome to our newsletter!<br />\
        Click <a href=\"{}\">here</a> to confirm your subscription.",
        confirmation_link
    );

    // Send a (useless) email to the new subscriber.
    // We are ignoring email delivery errors for now.
    email_client
        .send_email(new_subscriber.email, "Welcome!", &html_body, &plain_body)
        .await
}


#[tracing::instrument(
    name = "Saving new subscriber details in the database"
    skip(new_subscriber, transaction)
)]
pub async fn insert_subscriber(transaction: &mut Transaction<'_, Postgres>, new_subscriber: &NewSubscriber) -> Result<Uuid, sqlx::Error> {
    let subscriber_id = Uuid::new_v4();
    sqlx::query!(
        r#"
            INSERT INTO subscriptions (id, email, name, subscribed_at, status)
            VALUES ($1, $2, $3, $4, 'pending_confirmation')
        "#,
        subscriber_id,
        new_subscriber.email.as_ref(),
        new_subscriber.name.as_ref(),
        Utc::now()
    )
    .execute(transaction)
    .await
    .map_err(|e| {
        tracing::error!("Failed to execute query: {:?}", e);
        e
    // Using the `?` operator to return early
    // if the function failed, returning a sqlx::Error
    // We will talk about error handling in depth later!
    })?;
    Ok(subscriber_id)
}
