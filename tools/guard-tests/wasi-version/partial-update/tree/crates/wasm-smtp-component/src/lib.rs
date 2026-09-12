wit_bindgen::generate!({
    with: {
        "wasi:io/streams@0.2.12": wasi::io::streams,
        "wasi:sockets/tcp@0.2.11": wasi::sockets::tcp,
    },
});
