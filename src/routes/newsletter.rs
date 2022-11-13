use actix_web::{web, post, HttpResponse, HttpRequest, ResponseError};
use actix_web::http::StatusCode;
use sqlx::PgPool;
use crate::routes::error_chain_fmt;
use crate::email_client::EmailClient;
use crate::domain::SubscriberEmail;
use anyhow::Context;
use crate::authentication::{validate_credentials, basic_authentication, AuthError};


// Dummy implementation
#[tracing::instrument(
    name = "Publish a newsletter issue",
    skip(body, pool, email_client, request),
    // trace who is calling
    fields(username=tracing::field::Empty, user_id=tracing::field::Empty)
)]
#[post("/newsletter")]
pub async fn publish_newsletter(
    body: web::Json<BodyData>, pool: web::Data<PgPool>, email_client: web::Data<EmailClient>, request: HttpRequest,
) -> Result<HttpResponse, PublishError> {
    let credentials = basic_authentication(request.headers()).map_err(PublishError::AuthError)?;
    // tracing who is calling
    tracing::Span::current().record("username", &tracing::field::display(&credentials.username));

    let user_id = validate_credentials(credentials, &pool)
        .await
        .map_err(|e| match e {
            AuthError::InvalidCredentials(_) => PublishError::AuthError(e.into()),
            AuthError::UnexpectedError(_) => PublishError::UnexpectedError(e.into()),
        })?;

    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    let subscribers = get_confirmed_subscribers(&pool).await?;
    for subscriber in subscribers {
        match subscriber {
            Ok(subscriber) => {
                email_client
                    .send_email(
                        &subscriber.email,
                        &body.title,
                        &body.content.html,
                        &body.content.text,
                    )
                   .await
                    .with_context(|| {
                        format!("Failed to send newsletter to {}", subscriber.email)
                    })?;
            }
            Err(error) => {
                tracing::warn!(
                    // We record the error chain as a structured field on the log record.
                    error.cause_chain = ?error,
                    // Using `\` to split a long string literal over two lines
                    // without creating a `\n` character
                    "Skipping a confirmed subscriber. \
                    Their stored email are invalid",
                )
            }
        }
    } 
    return Ok(HttpResponse::Ok().finish());
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

struct ConfirmedSubscriber {
    email: SubscriberEmail,
}

#[tracing::instrument(name = "Get confirmed subscribers", skip(pool))]
async fn get_confirmed_subscribers(pool: &PgPool) -> Result< Vec< Result<ConfirmedSubscriber, anyhow::Error>>, anyhow::Error > {
    let rows = sqlx::query!(
        r#"
        SELECT email
        FROM subscriptions
        WHERE status = 'confirmed'
        "#,
    )
    .fetch_all(pool)
    .await?;
    // Map into the domain type
    let confirmed_subscribers = rows
        .into_iter()
        .map(|row| match SubscriberEmail::parse(row.email) {
            Ok(email) => return Ok(ConfirmedSubscriber { email }),
            Err(error) => return Err(anyhow::anyhow!(error))
        })
        .collect();

    return Ok(confirmed_subscribers);
}

#[derive(thiserror::Error)]
pub enum PublishError {
    #[error("Authentication failed.")]
    AuthError(#[source] anyhow::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

// Same logic to get the full error chain on `Debug` 
impl std::fmt::Debug for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return error_chain_fmt(self, f);
    }
}
impl ResponseError for PublishError {
    fn status_code(&self) -> StatusCode {
        match self {
            PublishError::UnexpectedError(_) => return StatusCode::INTERNAL_SERVER_ERROR,
            PublishError::AuthError(_) => StatusCode::UNAUTHORIZED,
        }
    }
}
