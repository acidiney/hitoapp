use crate::{Command, Error, Result, Snapshot, Source, SourceStop};
use std::{
    sync::{
        Mutex,
        mpsc::{Receiver, RecvTimeoutError},
    },
    time::Duration,
};
use tokio::sync::{oneshot, watch};

pub(crate) enum Order {
    Command(Command),
    Next(Box<Source>),
    CancelNext,
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
    requests: Receiver<Request>,
    stop: &OwnerStop,
    state: watch::Sender<Snapshot>,
    opened: oneshot::Sender<Result<()>>,
    activation: Receiver<()>,
) -> Result<()> {
    if stop.requested() {
        return Ok(());
    }
    let mut player = match hito_audio::Player::open(input) {
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
    let result = loop {
        if stop.requested() {
            break Ok(());
        }
        let snapshot = Snapshot::from_audio(player.snapshot());
        let error = snapshot.error;
        publish(&state, snapshot);
        if let Some(error) = error {
            break Err(error);
        }
        match requests.recv_timeout(Duration::from_millis(25)) {
            Ok(request) if !request.reply.is_closed() => {
                let result = handle(&mut player, request.order);
                publish(&state, Snapshot::from_audio(player.snapshot()));
                let _ = request.reply.send(result);
            }
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
            _ => (),
        }
    };
    let closed = player.close().map_err(Error::Native);
    if stop.requested() {
        Ok(())
    } else {
        result.and(closed)
    }
}

fn handle(player: &mut hito_audio::Player, order: Order) -> Result<bool> {
    match order {
        Order::Command(value) => command(player, value).map(|()| false),
        Order::Next(input) => player.prepare_next(*input).map_err(Error::Native),
        Order::CancelNext => player.cancel_next().map_err(Error::Native),
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
