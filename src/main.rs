use std::{env};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
use std::time::Duration;
use handlebars::Handlebars;
use lazy_static::lazy_static;
use kube::{Api, Client, Config};
use k8s_openapi::api::core::v1::{Secret, Service};
use serde::{Deserialize, Serialize};
use ssh2::Session;

lazy_static! {
    static ref SECRET_NAMESPACE: String = env::var("SECRET_NAMESPACE").unwrap_or("external-proxy".to_string());
    static ref SECRET_NAME: String = env::var("SECRET_NAME").unwrap_or("proxy-server-ssh-key".to_string());
    static ref PROXY_HOST: String = env::var("PROXY_HOST").unwrap_or("tiny.pizza".to_string());
    static ref PROXY_USER: String = env::var("PROXY_USER").unwrap_or("root".to_string());
}

static mut NGINX_CONFIG: String = String::new();

async fn generate_nginx_config(client: Client) -> String {
    let istio_service: Service = Api::namespaced(client, "istio-system").get("istio-ingress-gateway").await.unwrap();
    let service_ports = istio_service.spec.unwrap().ports.unwrap();
    let ip = public_ip::addr().await.expect("couldn't get ip ¯\\_(ツ)_/¯");
    let mut ports = Vec::<i32>::new();

    for service_port in service_ports {
        ports.push(service_port.port)
    }

    #[derive(Serialize, Deserialize)]
    struct Data {
        ports: Vec<i32>,
        ip: IpAddr,
    }

    let data = Data {
        ports,
        ip,
    };

    let mut hb = Handlebars::new();
    hb.register_template_file("config", "src/nginx.conf.tpl").unwrap();
    hb.render("config", &data).unwrap()
}

async fn start_ssh_session(client: Client) -> Result<Session, anyhow::Error> {
    println!("Looking for ssh-privatekey in {}/{}", *SECRET_NAMESPACE, *SECRET_NAME);
    let ssh_key_secret: Secret = Api::namespaced(client, &SECRET_NAMESPACE).get(&SECRET_NAME).await?;
    let ssh_key_bytes = ssh_key_secret.data.unwrap().get("ssh-privatekey").unwrap().clone().0;
    let ssh_key = String::from_utf8(ssh_key_bytes).unwrap();
    println!("Found ssh-privatekey");

    println!("Starting ssh session to {}", *PROXY_HOST);
    let tcp = TcpStream::connect(format!("{}:22", *PROXY_HOST)).unwrap();
    println!("Connected to proxy server: {}", *PROXY_HOST);

    let mut session = Session::new().unwrap();
    session.set_tcp_stream(tcp);
    session.handshake().unwrap();
    session.userauth_pubkey_memory("root", None, &ssh_key, None).unwrap();
    println!("SSH Authorized");
    println!("---");

    Ok(session)
}

fn reload_nginx(session: Session) -> Result<(), anyhow::Error> {
    let mut channel = session.channel_session()?;
    channel.exec("docker exec nginx-proxy nginx -s reload")?;
    print!("Nginx reloaded");
    Ok(())
}

fn write_config(nginx_config: String, session: &Session) -> Result<(), anyhow::Error> {
    println!("Attempting to write new config");
    let mut channel = session.scp_send(
        std::path::Path::new("/config/nginx/nginx.conf"),
        0o644,
        nginx_config.len() as u64,
        None,
    )?;
    channel.write_all(nginx_config.as_bytes())?;
    println!("Config changed to: \n```{}\n```", nginx_config);
    Ok(())
}

async fn recieve_remote_nginx_config(client: Client) -> Result<(), anyhow::Error> {
    let session = start_ssh_session(client.clone()).await?;
    let mut channel = session.scp_recv(
        std::path::Path::new("/config/nginx/nginx.conf"),
    )?.0;
    let mut s = String::new();
    channel.read_to_string(&mut s)?;

    unsafe { NGINX_CONFIG = s }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Creating k8s client...");
    let config = Config::infer().await?;
    println!("Config inferred: {}", config.cluster_url);
    let client = Client::try_from(config)?;
    println!("K8s client created");

    println!("---");

    unsafe {
        recieve_remote_nginx_config(client.clone()).await?;
        println!("Remote config found:\n{NGINX_CONFIG}\n---");
    }

    let nginx_config = generate_nginx_config(client.clone()).await;
    unsafe {
        if nginx_config == NGINX_CONFIG {
            println!("Matches proposed config")
        }
    }

    println!("Waiting for config changes...");

    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;

        let nginx_config = generate_nginx_config(client.clone()).await;
        unsafe {
            if nginx_config == NGINX_CONFIG {
                continue;
            }
            NGINX_CONFIG = nginx_config.clone();
        }

        let session = start_ssh_session(client.clone()).await?;

        write_config(nginx_config, &session)?;
        reload_nginx(session)?;
    }
}
