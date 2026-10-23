use super::protocol::{Answers, MAX_FRAME, Role, question, response};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::{JoinHandle, JoinSet},
};

const MAX_CONNECTIONS: usize = 8;
const MAX_QUERIES: usize = 8;

#[derive(Clone)]
pub(super) struct Question {
    pub connection: usize,
    pub record_type: u16,
}

#[derive(Default)]
struct State {
    accepted: usize,
    questions: Vec<Question>,
    answers: Option<Answers>,
    rounds: Option<Vec<Option<Answers>>>,
    answered: usize,
    peer_closed: BTreeSet<usize>,
    forced: bool,
    failed: bool,
}

pub(super) struct Report {
    pub answered: usize,
    pub accepted: usize,
    pub questions: Vec<Question>,
    pub peer_closed: BTreeSet<usize>,
    pub forced: bool,
    pub failed: bool,
    pub joined: bool,
}

pub(super) struct DnsFixture {
    pub address: SocketAddr,
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl DnsFixture {
    pub async fn start() -> Self {
        Self::configured(None, Role::Media, None).await
    }

    pub async fn for_api() -> Self {
        Self::configured(None, Role::Api, None).await
    }

    pub async fn api_rounds(rounds: Vec<Option<(std::net::Ipv4Addr, std::net::Ipv6Addr)>>) -> Self {
        assert!(!rounds.is_empty() && rounds.len() <= MAX_QUERIES / 2);
        let rounds = rounds
            .into_iter()
            .map(|answers| answers.map(|(v4, v6)| Answers { v4, v6 }))
            .collect();
        Self::configured(None, Role::Api, Some(rounds)).await
    }

    pub async fn answering(v4: std::net::Ipv4Addr, v6: std::net::Ipv6Addr) -> Self {
        Self::configured(Some(Answers { v4, v6 }), Role::Media, None).await
    }

    pub fn set_answers(&self, v4: std::net::Ipv4Addr, v6: std::net::Ipv6Addr) {
        self.state.lock().expect("the DNS state must lock").answers = Some(Answers { v4, v6 });
    }

    async fn configured(
        answers: Option<Answers>,
        role: Role,
        rounds: Option<Vec<Option<Answers>>>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned DNS listener must bind");
        let address = listener
            .local_addr()
            .expect("the owned DNS listener must have an address");
        let state = Arc::new(Mutex::new(State {
            answers,
            rounds,
            ..State::default()
        }));
        let changed = Arc::new(Notify::new());
        let serving = state.clone();
        let notifications = changed.clone();
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            let mut accepted = 0;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        if !matches!(result, Some(Ok(()))) {
                            serving.lock().expect("the DNS state must lock").failed = true;
                            break;
                        }
                    },
                    result = listener.accept(), if accepted < MAX_CONNECTIONS => {
                        let Ok((socket, _)) = result else {
                            serving.lock().expect("the DNS state must lock").failed = true;
                            break;
                        };
                        accepted += 1;
                        serving.lock().expect("the DNS state must lock").accepted = accepted;
                        let serving = serving.clone();
                        let changed = notifications.clone();
                        handlers.spawn(async move {
                            let result = tokio::time::timeout(Duration::from_secs(14), exchange(socket, accepted, role, &serving, &changed)).await;
                            if result.is_err() {
                                serving.lock().expect("the DNS state must lock").failed = true;
                            }
                        });
                    }
                }
            }
            let drained = tokio::time::timeout(Duration::from_secs(1), async {
                while let Some(result) = handlers.join_next().await {
                    if result.is_err() {
                        serving.lock().expect("the DNS state must lock").failed = true;
                    }
                }
            })
            .await;
            if drained.is_err() {
                serving.lock().expect("the DNS state must lock").forced = true;
                handlers.abort_all();
                while handlers.join_next().await.is_some() {}
            }
        });
        Self {
            address,
            state,
            changed,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub async fn questions(&self, count: usize) -> Vec<Question> {
        loop {
            let changed = self.changed.notified();
            let questions = self
                .state
                .lock()
                .expect("the DNS state must lock")
                .questions
                .clone();
            if questions.len() >= count {
                return questions;
            }
            changed.await;
        }
    }

    pub async fn peer_closed(&self, connections: &BTreeSet<usize>) {
        loop {
            let changed = self.changed.notified();
            if connections.is_subset(
                &self
                    .state
                    .lock()
                    .expect("the DNS state must lock")
                    .peer_closed,
            ) {
                return;
            }
            changed.await;
        }
    }

    pub async fn close(mut self) -> Report {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let joined = if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.is_ok(),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    false
                }
            }
        } else {
            false
        };
        let state = self.state.lock().expect("the DNS state must lock");
        Report {
            answered: state.answered,
            accepted: state.accepted,
            questions: state.questions.clone(),
            peer_closed: state.peer_closed.clone(),
            forced: state.forced,
            failed: state.failed,
            joined,
        }
    }
}

impl Drop for DnsFixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn exchange(
    mut socket: TcpStream,
    connection: usize,
    role: Role,
    state: &Mutex<State>,
    changed: &Notify,
) {
    loop {
        let mut length = [0_u8; 2];
        match socket.read_exact(&mut length).await {
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                state
                    .lock()
                    .expect("the DNS state must lock")
                    .peer_closed
                    .insert(connection);
                changed.notify_waiters();
                return;
            }
            Err(_) => {
                state.lock().expect("the DNS state must lock").failed = true;
                return;
            }
        }
        let length = usize::from(u16::from_be_bytes(length));
        assert!(
            (12..=MAX_FRAME).contains(&length),
            "the owned DNS frame must have a bounded length"
        );
        let mut bytes = vec![0_u8; length];
        socket
            .read_exact(&mut bytes)
            .await
            .expect("the complete owned DNS frame must arrive");
        let (record_type, question_end) = question(&bytes, role);
        let answers = {
            let mut state = state.lock().expect("the DNS state must lock");
            if state.questions.len() == MAX_QUERIES {
                state.failed = true;
                return;
            }
            let answers = match &state.rounds {
                Some(rounds) => {
                    let Some(answers) = rounds.get(state.questions.len() / 2).copied() else {
                        state.failed = true;
                        return;
                    };
                    answers
                }
                None => state.answers,
            };
            state.questions.push(Question {
                connection,
                record_type,
            });
            answers
        };
        changed.notify_waiters();
        if let Some(answers) = answers {
            let reply = response(&bytes, record_type, question_end, answers);
            socket
                .write_all(&reply)
                .await
                .expect("the bounded owned DNS answer must be sent");
            state.lock().expect("the DNS state must lock").answered += 1;
            changed.notify_waiters();
        }
    }
}
