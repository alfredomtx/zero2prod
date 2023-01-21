use anyhow::Context;
use secrecy::{Secret, ExposeSecret};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use actix_web::http::header::{HeaderMap};
use sqlx::PgPool;

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Invalid credentials.")]
    InvalidCredentials(#[source] anyhow::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

pub struct Credentials {
    pub username: String,
    pub password: Secret<String>,
}

// We extracted the db-querying logic in its own function with its own span.
#[tracing::instrument(name = "Get stored credentials", skip(username, pool))]
async fn get_stored_credentials(username: &str, pool: &PgPool) -> Result<Option<(uuid::Uuid, Secret<String>)>, anyhow::Error> {
    let row = sqlx::query!(
        r#"
        SELECT user_id, password_hash
        FROM users
        WHERE username = $1
        "#,
        username,
    )
    .fetch_optional(pool)
    .await
    .context("Failed to performed a query to retrieve stored credentials.")?
    .map(|row| (row.user_id, Secret::new(row.password_hash)));
    return Ok(row);
}


#[tracing::instrument(name = "Validate credentials", skip(credentials, pool))]
pub async fn validate_credentials(credentials: Credentials, pool: &PgPool) -> Result<uuid::Uuid, AuthError> {
    let row: Option<_> = sqlx::query!(
        r#"
        SELECT user_id, password_hash FROM USERS 
        WHERE username = $1
        "#,
        credentials.username
    )
    .fetch_optional(pool)
    .await
    .context("Failed to perform query to retrieve user credentials")
    .map_err(AuthError::UnexpectedError)?;
    
    let (expected_password_hash, user_id) = match row {
        Some(row) => (row.password_hash, row.user_id),
        None => {
            return Err(AuthError::InvalidCredentials(anyhow::anyhow!("Unknown username")));
        }
    };

    let expected_password_hash = PasswordHash::new(&expected_password_hash)
        .context("Failed to parse hash in PHC string format.")
        .map_err(AuthError::UnexpectedError)?;

    tracing::info_span!("Verify password hash")
        .in_scope(|| {
            Argon2::default()
                .verify_password(credentials.password.expose_secret().as_bytes(), &expected_password_hash)
        })
        .context("Invalid password")
        .map_err(AuthError::InvalidCredentials)?;

    return Ok(user_id);
}

pub fn basic_authentication(headers: &HeaderMap) -> Result<Credentials, anyhow::Error> {
    // the header value, if present, msut be a valid UTF8 string
    let header_value = headers
        .get("Authorization")
        .context("The 'Authorization' header was missing")?
        .to_str()
        .context("The 'Authorization' header was not a valid UFT8 string.")?;

    let base64encoded_segment = header_value
        .strip_prefix("Basic ")
        .context("The Authorization scheme was not 'Basic'.")?;
    let decoded_bytes = base64::decode_config(base64encoded_segment, base64::STANDARD)
        .context("Failed to decode base64 Basic crednetials.")?;
    let decoded_credentials = String::from_utf8(decoded_bytes)
        .context("Failed to decode credentials.")?;

    // Split into two segments, using : as delimiter
    let mut credentials = decoded_credentials.splitn(2, ':');
    let username = credentials
        .next()
        .ok_or_else(|| {
            anyhow::anyhow!("A username must be provided in Basic auth.")
        })?
        .to_string();
    let password = credentials
        .next()
        .ok_or_else(|| {
            anyhow::anyhow!("A password must be provided in Basic auth.")
        })?
        .to_string();

    return Ok(Credentials { username, password: Secret::new(password) });
}


#[tracing::instrument(name = "Change password", skip(password, pool))]
pub async fn change_password(
    user_id: uuid::Uuid,
    password: Secret<String>,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"
        UPDATE users
        SET password_hash = $1
        WHERE user_id = $2
        "#,
        password.expose_secret(),
        user_id
    )
    .execute(pool)
    .await
    .context("Failed to change user's password in the database.")?;
    return Ok(());
}