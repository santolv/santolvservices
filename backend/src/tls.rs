use axum::serve::Listener;
use std::{
    io,
    net::SocketAddr,
    path::Path,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time::{Instant, Sleep},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
    server::TlsStream,
};

pub fn config(
    cert: &Path,
    key: &Path,
) -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error>> {
    let certs = CertificateDer::pem_file_iter(cert)?.collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_file(key)?;
    let mut c = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
    c.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(c))
}
pub async fn serve(
    socket: TcpListener,
    config: Arc<rustls::ServerConfig>,
    app: axum::Router,
    shutdown: impl std::future::Future<Output = ()>,
) -> io::Result<()> {
    use hyper_util::{
        rt::{TokioIo, TokioTimer},
        service::TowerToHyperService,
    };
    let mut listener = TlsListener::new(socket, config);
    let mut tasks = JoinSet::new();
    let (stop, signal) = tokio::sync::watch::channel(false);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _=&mut shutdown=>break,
            Some(result)=tasks.join_next(),if !tasks.is_empty()=>{if let Err(error)=result{tracing::error!(%error,"HTTPS connection task failed");}}
            (io,peer)=listener.accept()=>{
                let app=app.clone().layer(axum::Extension(axum::extract::ConnectInfo(peer)));let mut signal=signal.clone();
                tasks.spawn(async move {
                    let mut builder=hyper::server::conn::http1::Builder::new();
                    builder.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(30)).max_buf_size(32*1024);
                    let connection=builder.serve_connection(TokioIo::new(io),TowerToHyperService::new(app));tokio::pin!(connection);
                    let result=tokio::select! {
                        result=&mut connection=>result,
                        _=signal.changed()=>{connection.as_mut().graceful_shutdown();connection.await}
                    };
                    if let Err(error)=result{tracing::debug!(%error,"HTTPS connection closed");}
                });
            }
        }
    }
    drop(listener);
    let _ = stop.send(true);
    while tasks.join_next().await.is_some() {}
    Ok(())
}
pub struct TlsListener {
    socket: TcpListener,
    acceptor: TlsAcceptor,
    pending: JoinSet<Option<(TlsIo, SocketAddr)>>,
    connections: Arc<Semaphore>,
}
impl TlsListener {
    pub fn new(socket: TcpListener, config: Arc<rustls::ServerConfig>) -> Self {
        Self {
            socket,
            acceptor: TlsAcceptor::from(config),
            pending: JoinSet::new(),
            connections: Arc::new(Semaphore::new(256)),
        }
    }
}
impl Listener for TlsListener {
    type Io = TlsIo;
    type Addr = SocketAddr;
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
    async fn accept(&mut self) -> (TlsIo, SocketAddr) {
        loop {
            tokio::select! {
                ready=self.pending.join_next(),if !self.pending.is_empty()=>{
                    if let Some(Ok(Some(accepted)))=ready{return accepted;}
                }
                incoming=self.socket.accept(),if self.pending.len()<64&&self.connections.available_permits()>0=>{
                    match incoming {
                        Ok((socket,peer))=>{
                            let Ok(permit)=self.connections.clone().try_acquire_owned() else{continue};
                            let acceptor=self.acceptor.clone();
                            self.pending.spawn(async move {
                                let _=socket.set_nodelay(true);
                                match tokio::time::timeout(Duration::from_secs(10),acceptor.accept(socket)).await {
                                    Ok(Ok(stream))=>Some((TlsIo{stream,_permit:permit,idle:Box::pin(tokio::time::sleep(Duration::from_secs(60)))},peer)),
                                    _=>None,
                                }
                            });
                        }
                        Err(e)=>{tracing::warn!(error=%e,"TLS accept failed");tokio::time::sleep(Duration::from_millis(100)).await;}
                    }
                }
                else=>{
                    if let Ok(permit)=self.connections.clone().acquire_owned().await{drop(permit);}
                }
            }
        }
    }
}
pub struct TlsIo {
    stream: TlsStream<TcpStream>,
    _permit: OwnedSemaphorePermit,
    idle: Pin<Box<Sleep>>,
}
impl TlsIo {
    fn expired(&mut self, cx: &mut Context<'_>) -> bool {
        self.idle.as_mut().poll(cx).is_ready()
    }
    fn progress(&mut self) {
        self.idle
            .as_mut()
            .reset(Instant::now() + Duration::from_secs(60));
    }
}
impl AsyncRead for TlsIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.expired(cx) {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        let before = buf.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        if matches!(result, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            self.progress();
        }
        result
    }
}
impl AsyncWrite for TlsIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.expired(cx) {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        let result = Pin::new(&mut self.stream).poll_write(cx, buf);
        if matches!(result,Poll::Ready(Ok(n)) if n>0) {
            self.progress();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.expired(cx) {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn tls_serves_while_another_client_stalls_handshake() {
        use axum::{Router, extract::ConnectInfo, routing::get};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let temp = tempfile::tempdir().unwrap();
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_path = temp.path().join("cert.pem");
        let key_path = temp.path().join("key.pem");
        std::fs::write(&cert_path, cert.pem()).unwrap();
        std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
        let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let config = config(&cert_path, &key_path).unwrap();
        let app = Router::new().route(
            "/peer",
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.ip().to_string() }),
        );
        let server = tokio::spawn(async move {
            serve(socket, config, app, std::future::pending())
                .await
                .unwrap();
        });
        let _stalled = TcpStream::connect(address).await.unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.der().clone()).unwrap();
        let c = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let test = async {
            let client = tokio_rustls::TlsConnector::from(Arc::new(c));
            let mut stream = client
                .connect(
                    "localhost".try_into().unwrap(),
                    TcpStream::connect(address).await.unwrap(),
                )
                .await
                .unwrap();
            stream
                .write_all(b"GET /peer HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).await.unwrap();
            let response = String::from_utf8(bytes).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"));
            assert!(response.ends_with("127.0.0.1"));
        };
        tokio::time::timeout(Duration::from_secs(3), test)
            .await
            .unwrap();
        server.abort();
    }
    #[test]
    fn rejects_invalid_pem() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("invalid");
        std::fs::write(&p, "not a certificate").unwrap();
        assert!(config(&p, &p).is_err());
    }
    #[test]
    fn rejects_mismatched_private_key() {
        let t = tempfile::tempdir().unwrap();
        let first = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let second = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = t.path().join("cert");
        let key = t.path().join("key");
        std::fs::write(&cert, first.cert.pem()).unwrap();
        std::fs::write(&key, second.signing_key.serialize_pem()).unwrap();
        assert!(config(&cert, &key).is_err());
    }
}
