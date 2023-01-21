use crate::email_client::EmailClient;
use crate::routes::{health_check, subscribe, confirm, publish_newsletter, admin_dashboard, log_out, change_password_form, change_password};
use actix_web::dev::Server;
use actix_web::web::{Data, self};
use actix_web::{App, HttpServer};
use sqlx::PgPool;
use std::net::TcpListener;
use tracing_actix_web::TracingLogger;
use crate::configuration::Settings;
use sqlx::postgres::PgPoolOptions;
use crate::configuration::DatabaseSettings;
use crate::routes::{home, login, login_form};
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_session::storage::RedisSessionStore;
use secrecy::{ExposeSecret, Secret};
use crate::authentication::reject_anonymous_users;
use actix_web_lab::middleware::from_fn;

// A new type to hold the newly built server and its port
pub struct Application {
    port: u16, 
    server: Server,
}

// We need to define a wrapper type in order to retrieve the URL
// in the `subscribe` handler.
// Retrieval from the context, in actix-web, is type-based: using
// a raw `String` would expose us to conflicts.
pub struct ApplicationBaseUrl(pub String);

impl Application {
    // We have converted the `build` function into a constructor for `Application`
    pub async fn build(configuration: Settings) -> Result<Self, anyhow::Error> {
        let connection_pool = get_connection_pool(&configuration.database);

        let sender_email = configuration
            .email_client
            .sender()
            .expect("Invalid sender email address.");

        let timeout = configuration.email_client.timeout();

        // Build an `EmailClient` using `configuration`
        let email_client = EmailClient::new(
            configuration.email_client.base_url,
            sender_email,
            configuration.email_client.authorization_token,
            timeout,
        );

        let address = format!("{}:{}"
            , configuration.application.host, configuration.application.port
        );
        // Bubble up the io::Error if we failed to bind the address
        // Otherwise call .await on our Server
        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();
        let server = run(
            listener,
            connection_pool,
            email_client,
            configuration.application.base_url,
            configuration.redis_uri,
        ).await?;

        // We "save" the bound port in one of `Application`'s fields
        return Ok(Self { port, server });
    }

    pub fn port(&self) -> u16 {
        return self.port;
    }

    // A more expressive name that makes it clear that
    // this function only returns when the application is stopped.
    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        return self.server.await;
    }
    
}



pub async fn run(
    listener: TcpListener,
    db_pool: PgPool,
    email_client: EmailClient,
    base_url: String,
    redis_uri: Secret<String>,
) -> Result<Server, anyhow::Error> {
    let db_pool = Data::new(db_pool);
    let email_client = Data::new(email_client);
    let base_url = Data::new(ApplicationBaseUrl(base_url));
    let secret_key = Key::from("test".as_bytes());
    let redis_store = RedisSessionStore::new(redis_uri.expose_secret()).await?;

    let server = HttpServer::new(move || {
        App::new()
            // TracingLogger instead of default actix_web logger to return with request_id (and other information aswell)
            .wrap(TracingLogger::default())
            .wrap(SessionMiddleware::new(redis_store.clone(), secret_key.clone()))

            .app_data(db_pool.clone())
            .app_data(email_client.clone())
            .app_data(base_url.clone())

            .service(home)
            .service(login)
            .service(login_form)
            .service(subscribe)
            .service(health_check)
            .service(confirm)
            .service(publish_newsletter)
            .service(web::scope("/admin")
                .wrap(from_fn(reject_anonymous_user))
                .route("/dashboard", web::post().to(admin_dashboard))
                .route("/password", web::get().to(change_password_form))
                .route("/password", web::post().to(change_password))
                .route("/logout", web::post().to(log_out))
            )
       
    })
    .listen(listener)?
    .run();

    return Ok(server);
}


pub fn get_connection_pool(configuration: &DatabaseSettings) -> PgPool {
    return PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy_with(configuration.with_db());
}

