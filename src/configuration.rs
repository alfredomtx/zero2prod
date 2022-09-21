use crate::domain::SubscriberEmail;
use config::{Config, ConfigError};
use secrecy::{ExposeSecret, Secret};
use serde_aux::field_attributes::deserialize_number_from_string;
use sqlx::postgres::PgConnectOptions;
use sqlx::postgres::PgSslMode;
use sqlx::ConnectOptions;
use std::env;
use url::Url;
use substring::Substring;


#[derive(Debug)]
#[derive(serde::Deserialize)]
#[derive(Clone)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    pub email_client: EmailClientSettings,
}

#[derive(Debug)]
#[derive(serde::Deserialize)]
#[derive(Clone)]
pub struct EmailClientSettings {
    pub timeout_milliseconds: u64,
    pub base_url: String,
    pub sender_email: String,
    pub authorization_token: Secret<String>,
}

impl EmailClientSettings {
    pub fn sender(&self) -> Result<SubscriberEmail, String> {
        SubscriberEmail::parse(self.sender_email.clone())
    }
    
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_milliseconds)
    }
}

#[derive(Debug)]
#[derive(serde::Deserialize)]   
#[derive(Clone)]
pub struct ApplicationSettings {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String
}

#[derive(Debug)]
#[derive(serde::Deserialize)]
#[derive(Clone)]
pub struct DatabaseSettings {
    pub username: String,
    pub password: Secret<String>,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub database_name: String,
    // Determine if we demand the connection to be encrypted or not
    pub require_ssl: bool
}

impl DatabaseSettings {
    pub fn without_db(&self) -> PgConnectOptions {
        let ssl_mode = if self.require_ssl {
            PgSslMode::Require
        } else {
            // Try an encrypted connection, fallback to unencrypted if it fails
            PgSslMode::Prefer
        };
        PgConnectOptions::new()
            .host(&self.host)
            .username(&self.username)
            .password(&self.password.expose_secret())
            .port(self.port)
            .ssl_mode(ssl_mode)
    }

    pub fn with_db(&self) -> PgConnectOptions {
        let mut options = self.without_db().database(&self.database_name);
        options.log_statements(tracing::log::LevelFilter::Trace); 
        options
    }
}

pub fn get_database_configuration_heroku() {
    let env_database_url = env::var("DATABASE_URL").expect("$DATABASE_URL is not set.");

    let database_url = Url::parse(env_database_url.as_str()).expect("Failed to parse $DATABASE_URL.");
    let db_port = database_url.port().expect("Failed to parse 'port' from database_url.");
    let db_host = database_url.host().expect("Failed to parse 'host' from database_url.");
    let db_password = database_url.password().expect("Failed to 'password' from database_url.");
    let db_username = database_url.username();
    let db_database_name = database_url.path().to_string();
    //  removes the first '/' from the path
    let db_database_name = db_database_name.substring(1, db_database_name.len());

    env::set_var("APP_DATABASE__PORT", db_port.to_string());
    env::set_var("APP_DATABASE__HOST", db_host.to_string());
    env::set_var("APP_DATABASE__USERNAME", db_username.to_string());
    env::set_var("APP_DATABASE__PASSWORD", db_password.to_string());
    env::set_var("APP_DATABASE__DATABASE_NAME", db_database_name.to_string());
}

pub fn set_port_heroku() {
    // Get the port from Heroku's `PORT` environment variable
    let port = env::var("PORT").expect("$PORT is not set.");
    env::set_var("APP_APPLICATION__PORT", port);
}

pub fn get_configuration() -> Result<Settings, ConfigError> {
    let base_path = env::current_dir().expect("Failed to determine the current directory");
    let configuration_directory = base_path.join("configuration");
    
    // Detect the running environment s
    // Default to `local` if unspecified
    let environment: Environment = env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "local".into())
        .try_into()
        .expect("Failed to parse APP_ENVIRONMENT.");
    
    let environment_filename = format!("{}.yaml", environment.as_str());

    let mut builder = Config::builder();

    // must re-assign to retain ownership
    builder = builder.add_source(config::File::from(configuration_directory.join("base.yaml")))
        .add_source(config::File::from(configuration_directory.join(&environment_filename)))
        // Add in settings from environment variables (with a prefix of APP and '__' as separator)
        // E.g. `APP_APPLICATION__PORT=5001 would set `Settings.application.port`
        .add_source(config::Environment::with_prefix("APP").prefix_separator("_").separator("__"));
    
    match environment {
        Environment::Production => {
            get_database_configuration_heroku();
            set_port_heroku();
        }
        _ => {}
    }

    let settings = builder.build()?;
    
    settings.try_deserialize::<Settings>()
}

/// The possible runtime environment for our application.
#[derive(Debug)]
#[derive(serde::Deserialize)]
pub enum Environment {
    Local,
    Production
}

impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Local => "local",
            Environment::Production => "production"
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            other => Err(format!(
                "{} is not a supported environment. Use either 'local' or 'production'.",
                other
            )),
        }
    }

}