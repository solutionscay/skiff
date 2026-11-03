//! Framed drops only reach the Host cache when every frame arrives in order.
#![cfg(unix)]

use std::path::PathBuf;

use skiff_client::host::{data, local::HostSelection};
use skiff_host_api::{Error, Request, Response};

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
    let path = data::drop_send(&c, "large.png", png.clone(), None).await.unwrap();
    let images = data::image_list(&c).await.unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].path, path);
    assert_eq!(images[0].size, png.len() as u64);

    // A first frame cannot start at another offset, and a total over the
    // Host limit is refused before anything enters the cache.
    for request in [
        Request::DropPut { name: "bad.png".into(), bytes: vec![1], upload: Some("out-of-order".into()), total: Some(2), offset: 1, finish: false },
        Request::DropPut { name: "huge.png".into(), bytes: vec![1], upload: Some("oversized".into()), total: Some(50 * 1024 * 1024 + 1), offset: 0, finish: false },
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
        })
        .await
        .unwrap();
    assert!(matches!(reply, Response::Path { path } if path.is_empty()));
    assert_eq!(data::image_list(&c).await.unwrap().len(), 1, "a partial upload is not cached");

    service.abort();
}
