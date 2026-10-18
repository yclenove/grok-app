use gcu_host_protocol::{
    approval::{host_callbacks, Office, Wait},
    auth::*,
    events::{Gallery, Role as ImageRole},
    lifecycle::Lifecycle,
    messages::*,
    server::*,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub fn png() -> Image {
    Image {
        mime: "image/png".into(),
        width: 1,
        height: 1,
        bytes: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
    }
}
pub struct Fixture {
    pub lifecycle: Lifecycle,
    pub gallery: Gallery,
    pub office: Office,
    pub callbacks: Arc<dyn HostCallbacks>,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
    root: PathBuf,
    pub peer: Peer,
}
impl Fixture {
    pub async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("gcu-life-{}", Secret::random().hex()));
        let gallery = Gallery::open(root.clone());
        let office = Office::open(None);
        let lifecycle = Lifecycle::new(office.clone(), gallery.clone());
        Self::connect(root, gallery, office, lifecycle).await
    }
    pub async fn replacement(&self) -> Self {
        Self::connect(
            self.root.clone(),
            self.gallery.clone(),
            self.office.clone(),
            self.lifecycle.clone(),
        )
        .await
    }
    async fn connect(
        root: PathBuf,
        gallery: Gallery,
        office: Office,
        lifecycle: Lifecycle,
    ) -> Self {
        let callbacks = lifecycle.observe(host_callbacks(office.clone()));
        let (a, b) = tokio::io::duplex(8192);
        let host = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
        let mut engine = host.clone();
        engine.role = Role::Engine;
        let secret = Secret::random();
        let other = secret.clone();
        let expected = host.instance.clone();
        let ep = expected.clone();
        let host_task =
            tokio::spawn(async move { Peer::connect(a, &host, &expected, &secret).await.unwrap() });
        let (engine, mut incoming, _) = Peer::connect(b, &engine, &ep, &other).await.unwrap();
        let (peer, _, _) = host_task.await.unwrap();
        callbacks.attached(ProcessInstance::current().unwrap(), peer.clone());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let task = tokio::spawn(async move {
            while let Some(request) = incoming.recv().await {
                seen.lock().unwrap().push(request.method.clone());
                let answer = if request.method == "engine.session.open" {
                    serde_json::to_value(SessionOpened {
                        mcp_url: "http://127.0.0.1:99/mcp".into(),
                        token: Secret::random().hex(),
                    })
                    .unwrap()
                } else if request.method == "engine.browser.sync" {
                    serde_json::json!({
                        "revision": request.params.get("revision").cloned().unwrap_or(serde_json::json!(0))
                    })
                } else {
                    serde_json::json!({})
                };
                engine.respond(&request, Ok(answer));
            }
        });
        Self {
            lifecycle,
            gallery,
            office,
            callbacks,
            requests,
            task,
            root,
            peer,
        }
    }
    pub async fn open(&self, id: &str) {
        self.lifecycle.open(id).await.unwrap();
    }
    pub fn calls(&self, method: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.as_str() == method)
            .count()
    }
    pub fn disconnect(&self) {
        self.peer.close();
        self.callbacks.disconnected();
    }
    pub fn request(&self, id: &str) -> RequestApp {
        RequestApp {
            session_id: id.into(),
            app: App {
                name: "Fixture".into(),
                pid: Some(7),
                icon: None,
                identity: Identity {
                    platform: Platform::Linux,
                    key: "fixture".into(),
                    image_path: "/fixture/editor".into(),
                    team_id: None,
                    publisher: None,
                    sha256: Some([7; 32]),
                    signature: Signature::Valid,
                },
            },
            reason: None,
            risk: Risk::Low,
        }
    }
    pub fn pending(&self, id: &str, request_id: u64) -> Wait {
        self.office
            .bind(request_id, &self.request(id), 0)
            .unwrap()
            .1
    }
    pub fn image(&self, session: &str, id: &str) -> PathBuf {
        self.gallery
            .admit(session, id, &png(), ImageRole::Full)
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.peer.close();
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
