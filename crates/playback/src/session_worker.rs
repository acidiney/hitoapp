use crate::{
    Command, Error, Result, Snapshot, Source, SourceStop, grants::Grants, preparation::Preparation,
};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, RecvTimeoutError, SyncSender},
    },
    time::Duration,
};
use tokio::sync::{oneshot, watch};

pub(crate) enum Order {
    Command(Command),
    Next(Box<Source>),
    CancelNext,
}

pub(crate) enum Message {
    Request(Request),
    Prepared(u64),
}

pub(crate) struct Mailbox {
    pub sender: SyncSender<Message>,
    pub receiver: Receiver<Message>,
}

pub(crate) struct Request {
    pub order: Order,
    pub reply: oneshot::Sender<Result<bool>>,
}

pub(crate) struct OwnerStop {
    input: SourceStop,
    output: Mutex<Option<hito_audio::Stop>>,
}
impl OwnerStop {
    pub fn new(input: SourceStop) -> Self {
        Self {
            input,
            output: Mutex::new(None),
        }
    }
    pub fn requested(&self) -> bool {
        self.input.requested()
    }
    pub fn input(&self) -> SourceStop {
        self.input.clone()
    }
    pub fn request(&self) {
        let output = self.output.lock().unwrap().clone();
        if let Some(output) = output {
            output.request();
        }
        self.input.request();
    }
    fn install(&self, stop: hito_audio::Stop) {
        *self.output.lock().unwrap() = Some(stop.clone());
        if self.requested() {
            stop.request();
        }
    }
}

pub(crate) fn run(
    input: Source,
    mailbox: Mailbox,
    stop: &OwnerStop,
    state: watch::Sender<Snapshot>,
    opened: oneshot::Sender<Result<()>>,
    activation: Receiver<()>,
    grants: Arc<Grants>,
) -> Result<()> {
    if stop.requested() {
        return Ok(());
    }
    let mut player = match crate::pcm::decode(input, &mut || stop.requested())
        .and_then(hito_audio::Player::open_decoder)
    {
        Ok(player) => player,
        Err(error) => {
            let _ = opened.send(Err(Error::Native(error)));
            return Err(Error::Native(error));
        }
    };
    stop.install(player.stop_handle());
    let mut initial = Snapshot::from_audio(player.snapshot());
    initial.status = crate::Status::Starting;
    publish(&state, initial);
    let _ = opened.send(Ok(()));
    loop {
        if stop.requested() {
            return Ok(());
        }
        match activation.recv_timeout(Duration::from_millis(25)) {
            Ok(()) => break,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => (),
        }
    }
    let mut preparation = Preparation::default();
    let result = loop {
        if stop.requested() {
            break Ok(());
        }
        preparation.poll(&mut player, &grants);
        let snapshot = Snapshot::from_audio(player.snapshot());
        let error = snapshot.error;
        publish(&state, snapshot);
        if let Some(error) = error {
            break Err(error);
        }
        match mailbox.receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(Message::Prepared(attempt)) => {
                preparation.wake(attempt, &mut player, &grants);
            }
            Ok(Message::Request(request)) if !request.reply.is_closed() => {
                let result = match request.order {
                    Order::Next(input) => {
                        preparation.start(*input, request.reply, mailbox.sender.clone());
                        continue;
                    }
                    Order::CancelNext => {
                        let pending = preparation.cancel();
                        player
                            .cancel_next()
                            .map_err(Error::Native)
                            .map(|cancelled| {
                                if cancelled {
                                    grants.withdraw();
                                }
                                pending || cancelled
                            })
                    }
                    Order::Command(value) => command(&mut player, value).map(|()| false),
                };
                publish(&state, Snapshot::from_audio(player.snapshot()));
                let _ = request.reply.send(result);
            }
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
            _ => (),
        }
    };
    preparation.cancel();
    let closed = player.close().map_err(Error::Native);
    if stop.requested() {
        Ok(())
    } else {
        result.and(closed)
    }
}

fn command(player: &mut hito_audio::Player, command: Command) -> Result<()> {
    let result = match command {
        Command::Resume => {
            if player.snapshot().ended {
                player.seek(Duration::ZERO).map_err(Error::Native)?;
            }
            player.resume()
        }
        Command::Pause => {
            player.pause();
            Ok(())
        }
        Command::Seek(position) => player.seek(position),
        Command::Volume(volume) => player.volume(volume),
        Command::Speed(speed) => player.speed(speed),
    };
    result.map_err(Error::Native)
}

pub(crate) fn publish(state: &watch::Sender<Snapshot>, next: Snapshot) {
    state.send_if_modified(|previous| {
        if *previous == next {
            false
        } else {
            *previous = next;
            true
        }
    });
}
