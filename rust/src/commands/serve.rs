use serde::{Deserialize, Serialize};
use serde_json;
use tiny_http::{Method, Response, Server};

#[derive(Serialize, Deserialize)]
struct RunParams {
    repo: String,
    stat: String,
    since: Option<String>,
}

fn health_check() -> String {
    serde_json::json!({"status": "OK"}).to_string()
}

fn list_repos() -> String {
    // Implement logic to list all repos and their stats
    serde_json::json!({"message": "Not implemented yet"}).to_string()
}

fn run_stat(body: String) -> impl Iterator<Item = String> {
    let params: RunParams = serde_json::from_str(&body).unwrap_or_else(|_| RunParams {
        repo: String::new(),
        stat: String::new(),
        since: None,
    });

    // This iterator simulates streaming data
    // Replace this with your actual data processing logic
    (0..10).map(move |i| {
        serde_json::json!({
            "chunk": i,
            "repo": params.repo,
            "stat": params.stat,
            "data": format!("Data chunk {}", i)
        })
        .to_string()
            + "\n"
    })
}

pub fn serve_command(port: &str) {
    let port = port.parse::<u16>().expect("Invalid port number");
    let addr = format!("127.0.0.1:{}", port);

    println!("Starting server on {}", addr);

    let server = Server::http(&addr).unwrap();

    for mut request in server.incoming_requests() {
        match (request.method(), request.url()) {
            (Method::Get, "/health") => {
                let response = Response::from_string(health_check());
                request.respond(response).unwrap();
            }
            (Method::Get, "/repos") => {
                let response = Response::from_string(list_repos());
                request.respond(response).unwrap();
            }
            (Method::Post, "/run") => {
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                //Todo parse the body as JSON and build a Measurement from that

                let mut writer = request.into_writer();

                for chunk in run_stat(body) {
                    writer.write_all(chunk.as_bytes()).unwrap();
                }
                writer.flush().unwrap();
            }
            _ => {
                let response = Response::from_string("Not Found").with_status_code(404);
                request.respond(response).unwrap();
            }
        }
    }
}
