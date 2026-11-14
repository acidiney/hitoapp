use crate::links::video_id;

#[test]
fn own_links_yield_only_the_canonical_video_id() {
    for reference in [
        "https://www.youtube.com/watch?v=HitoLink001",
        "https://youtube.com/watch?v=HitoLink001&t=42s",
        "https://youtube.com:443/watch?v=HitoLink001",
        "http://youtube.com:80/watch?v=HitoLink001",
        "https://m.youtube.com/watch?list=PL1&v=HitoLink001",
        "https://music.youtube.com/watch?v=HitoLink001&si=owned",
        "http://www.youtube.com/watch?v=HitoLink001#t=3",
        "www.youtube.com/watch?v=HitoLink001",
        "https://youtu.be/HitoLink001",
        "https://youtu.be/HitoLink001?si=owned",
        "youtu.be/HitoLink001",
        "https://www.youtube.com/shorts/HitoLink001",
        "https://youtube.com/shorts/HitoLink001/",
        "HitoLink001",
        "  https://WWW.YOUTUBE.COM/watch?v=HitoLink001  ",
    ] {
        assert_eq!(
            video_id(reference).as_deref(),
            Some("HitoLink001"),
            "{reference}"
        );
    }
}

#[test]
fn foreign_malformed_and_hostile_references_are_not_owned() {
    let overlong = format!(
        "https://www.youtube.com/watch?v=HitoLink001&pad={}",
        "a".repeat(2048)
    );
    for reference in [
        "",
        "   ",
        "https://evil.example/watch?v=HitoLink001",
        "https://www.youtube.com.evil.example/watch?v=HitoLink001",
        "https://evil.example/?u=https://www.youtube.com/watch?v=HitoLink001",
        "https://user@www.youtube.com/watch?v=HitoLink001",
        "https://user:secret@youtu.be/HitoLink001",
        "https://www.youtube.com:8443/watch?v=HitoLink001",
        "http://www.youtube.com:443/watch?v=HitoLink001",
        "https://youtu.be:80/HitoLink001",
        "ftp://www.youtube.com/watch?v=HitoLink001",
        "javascript:alert(1)",
        "https://www.youtube.com/watch",
        "https://www.youtube.com/watch?v=short",
        "https://www.youtube.com/watch?v=HitoLink0012",
        "https://www.youtube.com/watch?v=Hito%20ink001",
        "https://www.youtube.com/watch?v=HitoLink001&v=HitoLink002",
        "https://www.youtube.com/playlist?list=HitoLink001",
        "https://www.youtube.com/channel/HitoLink001",
        "https://music.youtube.com/shorts/HitoLink001",
        "https://youtu.be/HitoLink001/extra",
        "https://youtu.be/",
        "https://www.youtu.be/HitoLink001",
        "https://127.0.0.1/watch?v=HitoLink001",
        "https://www.youtube.com/watch?v=HitoLink001\n",
        "Hito\tink001",
        "HitoLink\u{7f}01",
        "HitoLink!01",
        overlong.as_str(),
    ] {
        assert_eq!(video_id(reference), None, "{reference:?}");
    }
}
