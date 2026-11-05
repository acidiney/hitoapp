use hito_core::{
    AuthSession, CoreConfig, Credentials, DeleteAccount, Error, HostDeviceId, Services, open,
};
use image::{ImageEncoder, Rgb, RgbImage};

const PASSWORD: &str = "the first long password";
const MAX_UPLOAD: usize = 2 * 1024 * 1024;

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

struct World {
    _directory: tempfile::TempDir,
    hito: Services,
    owner: AuthSession,
    guest: AuthSession,
    other: AuthSession,
}

async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("avatar").unwrap(),
    ))
    .await
    .unwrap();
    let owner = hito.identity.setup(credentials("owner")).await.unwrap();
    let actor = hito.identity.authenticate(&owner.token).await.unwrap();
    for name in ["guest", "other"] {
        hito.identity
            .create_user(&actor, credentials(name))
            .await
            .unwrap();
    }
    let guest = hito.identity.login(credentials("guest")).await.unwrap();
    let other = hito.identity.login(credentials("other")).await.unwrap();
    World {
        _directory: directory,
        hito,
        owner,
        guest,
        other,
    }
}

fn picture(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        if x * 3 >= width && x * 3 < width * 2 {
            Rgb([20, 200, 40])
        } else {
            Rgb([200 - (y % 50) as u8, 30, 60])
        }
    })
}

fn jpeg(image: &RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    bytes
}

fn png(image: &RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    bytes
}

fn webp(image: &RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    bytes
}

fn with_exif(jpeg: &[u8]) -> Vec<u8> {
    let payload = b"Exif\0\0GPSLatitude SECRETGPS 51.5072 camera SECRETCAM";
    let mut bytes = jpeg[..2].to_vec();
    bytes.extend([0xff, 0xe1]);
    bytes.extend(((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend(payload);
    bytes.extend(&jpeg[2..]);
    bytes
}

fn with_png_text(png: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    let body = b"Comment\0SECRETTEXT home address";
    chunk.extend((body.len() as u32).to_be_bytes());
    let mut checked = b"tEXt".to_vec();
    checked.extend(body);
    chunk.extend(&checked);
    chunk.extend(crc(&checked).to_be_bytes());
    let mut bytes = png[..33].to_vec();
    bytes.extend(chunk);
    bytes.extend(&png[33..]);
    bytes
}

fn crc(data: &[u8]) -> u32 {
    let mut value = 0xffff_ffffu32;
    for byte in data {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ 0xedb8_8320
            } else {
                value >> 1
            };
        }
    }
    !value
}

fn png_claiming(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = png(&picture(8, 8));
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    let checksum = crc(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&checksum.to_be_bytes());
    bytes
}

async fn actor(world: &World, session: &AuthSession) -> hito_core::Actor {
    world
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[tokio::test]
async fn jpeg_png_and_webp_uploads_become_one_square_jpeg() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    for upload in [
        jpeg(&picture(640, 360)),
        png(&picture(300, 500)),
        webp(&picture(400, 400)),
    ] {
        let user = world
            .hito
            .identity
            .set_avatar(&actor, upload)
            .await
            .unwrap();
        let version = user.avatar_version.expect("an avatar version");
        let avatar = world.hito.identity.avatar(&actor, &user.id).await.unwrap();
        assert_eq!(avatar.version, version);
        assert_eq!(avatar.content_type, "image/jpeg");
        let decoded =
            image::load_from_memory_with_format(&avatar.bytes, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
    }
}

#[tokio::test]
async fn wide_pictures_are_cropped_around_the_centre() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    let user = world
        .hito
        .identity
        .set_avatar(&actor, png(&picture(900, 300)))
        .await
        .unwrap();
    let avatar = world.hito.identity.avatar(&actor, &user.id).await.unwrap();
    let decoded = image::load_from_memory(&avatar.bytes).unwrap().to_rgb8();
    let centre = decoded.get_pixel(128, 128);
    assert!(centre[1] > 150 && centre[0] < 80, "{centre:?}");
    let edge = decoded.get_pixel(2, 128);
    assert!(
        edge[1] > 150 && edge[0] < 80,
        "centre crop keeps only the middle third: {edge:?}"
    );
}

#[tokio::test]
async fn the_format_comes_from_the_bytes_not_from_a_declaration() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    for upload in [
        b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00;".to_vec(),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        b"just text pretending to be image/png".to_vec(),
        Vec::new(),
        vec![0xff, 0xd8, 0xff],
    ] {
        let error = world
            .hito
            .identity
            .set_avatar(&actor, upload)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{error:?}");
    }
    assert_eq!(
        world
            .hito
            .identity
            .authenticate(&world.guest.token)
            .await
            .unwrap()
            .user()
            .avatar_version,
        None
    );
}

#[tokio::test]
async fn uploads_over_two_mebibytes_are_refused() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    let mut big = jpeg(&picture(64, 64));
    big.resize(MAX_UPLOAD + 1, 0);
    let error = world
        .hito
        .identity
        .set_avatar(&actor, big)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Invalid(_)), "{error:?}");
}

#[tokio::test]
async fn pictures_that_claim_huge_dimensions_are_refused_before_decoding() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    for (width, height) in [(30_000, 30_000), (65_000, 8), (5_000, 5_000)] {
        let error = world
            .hito
            .identity
            .set_avatar(&actor, png_claiming(width, height))
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Invalid(_)),
            "{width}x{height}: {error:?}"
        );
    }
}

#[tokio::test]
async fn stored_pictures_carry_no_metadata() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    for upload in [
        with_exif(&jpeg(&picture(300, 300))),
        with_png_text(&png(&picture(300, 300))),
    ] {
        assert!(contains(&upload, b"SECRET"));
        let user = world
            .hito
            .identity
            .set_avatar(&actor, upload)
            .await
            .unwrap();
        let avatar = world.hito.identity.avatar(&actor, &user.id).await.unwrap();
        assert!(!contains(&avatar.bytes, b"SECRET"));
        assert!(!contains(&avatar.bytes, b"Exif"));
        assert!(!contains(&avatar.bytes, b"GPS"));
        assert!(!avatar.bytes.windows(2).any(|pair| pair == [0xff, 0xe1]));
    }
}

#[tokio::test]
async fn replacing_keeps_one_picture_and_removing_clears_it() {
    let world = world().await;
    let actor = actor(&world, &world.guest).await;
    let first = world
        .hito
        .identity
        .set_avatar(&actor, jpeg(&picture(300, 300)))
        .await
        .unwrap();
    let second = world
        .hito
        .identity
        .set_avatar(&actor, png(&picture(200, 300)))
        .await
        .unwrap();
    assert_ne!(first.avatar_version, second.avatar_version);
    let removed = world.hito.identity.remove_avatar(&actor).await.unwrap();
    assert_eq!(removed.avatar_version, None);
    assert!(matches!(
        world.hito.identity.avatar(&actor, &actor.user().id).await,
        Err(Error::NotFound)
    ));
    assert!(world.hito.identity.remove_avatar(&actor).await.is_ok());
}

#[tokio::test]
async fn only_the_owner_and_administrators_can_read_a_picture() {
    let world = world().await;
    let guest = actor(&world, &world.guest).await;
    let admin = actor(&world, &world.owner).await;
    let other = actor(&world, &world.other).await;
    let user = world
        .hito
        .identity
        .set_avatar(&guest, jpeg(&picture(300, 300)))
        .await
        .unwrap();
    assert!(world.hito.identity.avatar(&guest, &user.id).await.is_ok());
    assert!(world.hito.identity.avatar(&admin, &user.id).await.is_ok());
    assert!(matches!(
        world.hito.identity.avatar(&other, &user.id).await,
        Err(Error::NotFound)
    ));
    let listed = world.hito.identity.users(&admin).await.unwrap();
    let entry = listed.iter().find(|entry| entry.id == user.id).unwrap();
    assert_eq!(entry.avatar_version, user.avatar_version);
    let other_entry = listed
        .iter()
        .find(|entry| entry.username == "other")
        .unwrap();
    assert_eq!(other_entry.avatar_version, None);
}

#[tokio::test]
async fn deleting_an_account_removes_its_picture() {
    let world = world().await;
    let guest = actor(&world, &world.guest).await;
    let admin = actor(&world, &world.owner).await;
    world
        .hito
        .identity
        .set_avatar(&guest, jpeg(&picture(300, 300)))
        .await
        .unwrap();
    world
        .hito
        .identity
        .delete_account(
            &admin,
            &guest.user().id,
            DeleteAccount {
                username: "guest".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        world.hito.identity.avatar(&admin, &guest.user().id).await,
        Err(Error::NotFound)
    ));
}
