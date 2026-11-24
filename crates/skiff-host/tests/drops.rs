//! Framed drops only reach the Host cache when every frame arrives in order.
//! A file of several frames goes both ways, keeps its session, and an
//! Insert touches it on the Host (#169).
#![cfg(unix)]

use std::path::PathBuf;

use skiff_client::host::{data, local::HostSelection};
use skiff_host_api::{Error, HostWho, Request, Response, DATA_CHUNK};

mod common;
use common::{private_home, start, Guard};

#[tokio::test(flavor = "multi_thread")]
async fn framed_drops_are_ordered_bounded_and_atomic() {
    let dir = PathBuf::from("/tmp").join(format!("skiff-host-drops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _guard = Guard(dir.clone());
    private_home(&dir);
    let session = dir.join("skiffd.sock");
    skiff_core::state::set_socket(session.clone());
    let hello = skiff_host::hello().unwrap();
    let service = start(&skiff_core::socket::data_socket(&session), hello.clone());
    let selection = HostSelection::new(session, dir.join("bootstrap-id"));
    let plane = selection.connect_data(&hello.host.host_id).await.unwrap();
    let c = plane.connection();

    // `drop_send` splits this PNG into two ordered frames. The cache appears
    // only when its final byte arrives.
    let mut png = vec![0; 4 * 1024 * 1024 + 1];
    png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let path = data::drop_send(&c, "large.png", png.clone(), None, None).await.unwrap();
    let images = data::image_list(&c).await.unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].path, path);
    assert_eq!(images[0].size, png.len() as u64);

    // A first frame cannot start at another offset, and a total over the
    // Host limit is refused before anything enters the cache.
    for request in [
        Request::DropPut { name: "bad.png".into(), bytes: vec![1], upload: Some("out-of-order".into()), total: Some(2), offset: 1, finish: false, who: None },
        Request::DropPut { name: "huge.png".into(), bytes: vec![1], upload: Some("oversized".into()), total: Some(100 * 1024 * 1024 + 1), offset: 0, finish: false, who: None },
    ] {
        assert!(matches!(c.request(request).await, Err(Error::Invalid { .. })));
    }

    // A valid first frame has no cache side effect. If its final frame is
    // never sent, it remains only in the service's in-memory upload map.
    let reply = c
        .request(Request::DropPut {
            name: "partial.png".into(),
            bytes: b"\x89PNG\r\n\x1a\nfirst".to_vec(),
            upload: Some("partial".into()),
            total: Some(32),
            offset: 0,
            finish: false,
            who: None,
        })
        .await
        .unwrap();
    assert!(matches!(reply, Response::Path { path } if path.is_empty()));
    assert_eq!(data::image_list(&c).await.unwrap().len(), 1, "a partial upload is not cached");

    // drops@2: the limit is the Host's `cache_max_mb` (100 MB by default),
    // not 50 MB. A 60 MB upload's first frame is taken.
    let reply = c
        .request(Request::DropPut { name: "big.bin".into(), bytes: vec![0; 8], upload: Some("big".into()), total: Some(60 * 1024 * 1024), offset: 0, finish: false, who: None })
        .await;
    assert!(matches!(reply, Ok(Response::Path { .. })), "{reply:?}");

    // A file of three frames, with its session, goes to the Host and back
    // byte for byte through ranged reads.
    let doc: Vec<u8> = (0..2 * DATA_CHUNK + 5).map(|i| (i % 251) as u8).collect();
    let src = dir.join("notes.txt");
    std::fs::write(&src, &doc).unwrap();
    let f = data::drop_file(&c, &src, None).unwrap();
    let who = HostWho { session: "s1".into(), title: "claude".into(), project: Some("web".into()) };
    let mut frames = 0;
    let kept = data::drop_send_file(&c, &f, Some(who.clone()), None, |_| frames += 1).await.unwrap();
    assert_eq!(frames, 3);
    assert!(kept.ends_with("/notes.txt"), "{kept}");
    let copy = dir.join("copy.txt");
    data::image_fetch(&c, &kept, &copy).await.unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), doc);
    let entry = data::image_list(&c).await.unwrap().into_iter().find(|i| i.path == kept).unwrap();
    assert_eq!((entry.session.as_deref(), entry.title.as_deref(), entry.project.as_deref()), (Some("s1"), Some("claude"), Some("web")));

    // An Insert into another session touches the file on the Host: a new
    // mtime, so a flush keeps it, and the new session line.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 24 * 60 * 60);
    std::fs::File::options().write(true).open(&kept).unwrap().set_modified(old).unwrap();
    let other = HostWho { session: "s2".into(), title: "codex".into(), project: None };
    assert!(data::image_touch(&c, vec![kept.clone()], Some(other)).await.unwrap());
    let mtime = std::fs::metadata(&kept).unwrap().modified().unwrap();
    assert!(mtime.elapsed().unwrap() < std::time::Duration::from_secs(60), "touched");
    let entry = data::image_list(&c).await.unwrap().into_iter().next().unwrap();
    assert_eq!((entry.path.as_str(), entry.session.as_deref()), (kept.as_str(), Some("s2")));
    // Pin answers ok over the wire.
    data::image_pin(&c, vec![kept.clone()], true).await.unwrap();

    service.abort();
}
