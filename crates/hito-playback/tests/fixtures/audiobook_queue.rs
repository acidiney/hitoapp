use crate::{fixture, support};
use hito_core::{Action, Actor, Credentials, Search};
use serde_json::{Value, json};
use uuid::Uuid;

pub struct Library {
    pub source: fixture::LocalPlayback,
    pub actor: Actor,
    pub recordings: [Uuid; 2],
    pub book: Uuid,
    pub chapters: [Uuid; 3],
    pub anchors: Vec<Value>,
    pub second_path: std::path::PathBuf,
    pub second_original: Vec<u8>,
}

impl Library {
    pub async fn start() -> Self {
        let f = fixture::LocalPlayback::start().await;
        let second_path = f.music.join("second.wav");
        let second_original = support::write_wav(&second_path, 200);
        support::scan(&f.services, &f.actor, &f.library).await;
        let credentials = Credentials {
            username: "chapter-listener".into(),
            password: "private chapter listener password".into(),
        };
        let user = f
            .services
            .identity
            .create_user(
                &f.actor,
                Credentials {
                    username: credentials.username.clone(),
                    password: credentials.password.clone(),
                },
            )
            .await
            .unwrap();
        f.services
            .identity
            .set_library_access(&f.actor, &f.library, &user.id, true)
            .await
            .unwrap();
        let session = f.services.identity.login(credentials).await.unwrap();
        let actor = f
            .services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        assert!(!actor.user().admin);
        let catalog = f
            .services
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(catalog.total, 2);
        let a = Uuid::parse_str(&f.recording).unwrap();
        let b = catalog
            .items
            .iter()
            .find(|recording| recording.id != f.recording)
            .unwrap()
            .id
            .parse::<Uuid>()
            .unwrap();
        let book = Uuid::new_v4();
        let chapters = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let mut actions = vec![Action::PlaylistCreate {
            playlist_id: book,
            name: "Private repeated audiobook chapters".into(),
        }];
        for (index, recording_id) in [a, b, a].into_iter().enumerate() {
            actions.push(Action::EntryInsert {
                playlist_id: book,
                entry_id: chapters[index],
                recording_id,
                position: index as i64,
            });
        }
        actions.push(Action::AudiobookCreate { playlist_id: book });
        support::synchronize(&f.services, &actor, actions).await;
        let anchors = [a, b, a].into_iter().zip(chapters).map(|(recording, entry)| {
            json!({"audiobook_id": book, "entry_id": entry, "recording_id": recording})
        }).collect::<Vec<_>>();
        Self {
            source: f,
            actor,
            recordings: [a, b],
            book,
            chapters,
            anchors,
            second_path,
            second_original,
        }
    }
}
