use crate::{Error, PrincipalId, Result};
use aw_store::{
    AuthSessionInfo, OAuthConnectorInfo, OAuthTokenSet, OidcProviderInfo, SessionStore,
    VerifiedOidcLogin,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ring::signature;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::{form_urlencoded, Url};
use zeroize::{Zeroize, Zeroizing};

const MAX_HTTP_BODY: usize = 256 * 1024;
const MAX_CALLBACK_HEAD: usize = 16 * 1024;
const MAX_JWT_BYTES: usize = 64 * 1024;
const CLOCK_SKEW_SECONDS: i64 = 120;
const CHATGPT_ISSUER: &str = "https://auth.openai.com";
const CHATGPT_AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const CHATGPT_TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const CHATGPT_RESOURCE: &str = "https://api.openai.com/v1";
const CHATGPT_DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const CHATGPT_SCOPE: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

#[derive(Debug)]
pub struct OidcFlowIssued {
    pub login: VerifiedOidcLogin,
    pub session: AuthSessionInfo,
    pub refresh_token: Zeroizing<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatGptPlanModel {
    pub slug: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    revocation_endpoint: Option<String>,
    #[serde(default)]
    response_types_supported: Vec<String>,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}
#[derive(Clone, Debug, Deserialize)]
struct Jwk {
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    alg: Option<String>,
    #[serde(rename = "use", default)]
    use_: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    #[serde(default)]
    crv: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    error: Option<String>,
}
impl Drop for TokenResponse {
    fn drop(&mut self) {
        if let Some(v) = self.access_token.as_mut() {
            v.zeroize();
        }
        if let Some(v) = self.refresh_token.as_mut() {
            v.zeroize();
        }
        if let Some(v) = self.id_token.as_mut() {
            v.zeroize();
        }
    }
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    typ: Option<String>,
}
#[derive(Debug, Deserialize)]
struct IdClaims {
    iss: String,
    sub: String,
    aud: serde_json::Value,
    exp: i64,
    iat: i64,
    #[serde(default)]
    nbf: Option<i64>,
    nonce: String,
    #[serde(default)]
    azp: Option<String>,
}

struct CodeGrant {
    code: Zeroizing<String>,
    verifier: Zeroizing<String>,
    nonce: Option<Zeroizing<String>>,
    redirect_uri: String,
}

fn random_b64(bytes: usize) -> Result<Zeroizing<String>> {
    let mut raw = vec![0u8; bytes];
    getrandom::fill(&mut raw).map_err(|_| Error::Config("OS randomness unavailable".into()))?;
    let encoded = URL_SAFE_NO_PAD.encode(&raw);
    raw.zeroize();
    Ok(Zeroizing::new(encoded))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn new_chatgpt_connector_id() -> Result<String> {
    let random = random_b64(18)?;
    Ok(format!("chatgpt-{}", random.as_str()))
}

pub fn new_chatgpt_host_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Config("OS randomness unavailable".into()))?;
    // RFC 4122 UUIDv4 / RFC 9562 compatible variant bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "urn:uuid:{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    ))
}

fn authority(addr: SocketAddr) -> String {
    match addr.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", addr.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", addr.port()),
    }
}

fn safe_endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value)
        .map_err(|_| Error::Config("identity/OAuth endpoint must be an absolute URL".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::Config("identity/OAuth endpoint requires a host".into()))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|a| a.is_loopback());
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(Error::Denied(
            "identity/OAuth endpoint contains forbidden authority or fragment data",
        ));
    }
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(Error::Denied(
            "identity/OAuth endpoints require HTTPS except loopback development endpoints",
        ));
    }
    Ok(url)
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Error::Transport("cannot initialize identity HTTP client"))
}

async fn bounded_bytes(response: reqwest::Response) -> Result<Zeroizing<Vec<u8>>> {
    if !response.status().is_success() {
        return Err(Error::Http {
            status: response.status().as_u16(),
            hint: "identity/OAuth endpoint rejected request",
        });
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_HTTP_BODY as u64)
    {
        return Err(Error::Limit("identity/OAuth response exceeds 256 KiB"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| Error::Transport("identity/OAuth response read failed"))?;
    if bytes.len() > MAX_HTTP_BODY {
        return Err(Error::Limit("identity/OAuth response exceeds 256 KiB"));
    }
    Ok(Zeroizing::new(bytes.to_vec()))
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(http: &reqwest::Client, url: &str) -> Result<T> {
    let endpoint = safe_endpoint(url)?;
    let response = http
        .get(endpoint)
        .send()
        .await
        .map_err(|_| Error::Transport("identity/OAuth GET failed"))?;
    let bytes = bounded_bytes(response).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::Protocol("identity/OAuth endpoint returned invalid JSON"))
}

async fn discovery(http: &reqwest::Client, provider: &OidcProviderInfo) -> Result<Discovery> {
    let issuer = safe_endpoint(&provider.issuer)?;
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.as_str().trim_end_matches('/')
    );
    let value: Discovery = fetch_json(http, &url).await?;
    if value.issuer != provider.issuer {
        return Err(Error::Denied("OIDC discovery issuer mismatch"));
    }
    safe_endpoint(&value.authorization_endpoint)?;
    safe_endpoint(&value.token_endpoint)?;
    safe_endpoint(&value.jwks_uri)?;
    if !value.response_types_supported.is_empty()
        && !value.response_types_supported.iter().any(|v| v == "code")
    {
        return Err(Error::Denied(
            "OIDC provider does not advertise authorization-code flow",
        ));
    }
    if !value.code_challenge_methods_supported.is_empty()
        && !value
            .code_challenge_methods_supported
            .iter()
            .any(|v| v == "S256")
    {
        return Err(Error::Denied("OIDC provider does not advertise PKCE S256"));
    }
    Ok(value)
}

async fn prepare_code_flow(
    authorization_endpoint: &str,
    client_id: &str,
    scopes: &BTreeSet<String>,
    listen: SocketAddr,
    timeout: Duration,
    with_nonce: bool,
    on_url: impl FnOnce(&str),
) -> Result<CodeGrant> {
    if !listen.ip().is_loopback() {
        return Err(Error::Denied(
            "OIDC/OAuth callback listener must bind loopback",
        ));
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let local = listener.local_addr()?;
    let redirect_uri = format!("http://{}/callback", authority(local));
    let state = random_b64(32)?;
    let verifier = random_b64(32)?;
    let nonce = if with_nonce {
        Some(random_b64(32)?)
    } else {
        None
    };
    let mut url = safe_endpoint(authorization_endpoint)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair(
                "scope",
                &scopes.iter().cloned().collect::<Vec<_>>().join(" "),
            )
            .append_pair("state", state.as_str())
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256");
        if let Some(nonce) = nonce.as_ref() {
            q.append_pair("nonce", nonce.as_str());
        }
    }
    on_url(url.as_str());
    let code = tokio::time::timeout(
        timeout,
        wait_callback(listener, &authority(local), state.as_str()),
    )
    .await
    .map_err(|_| Error::Timeout)??;
    Ok(CodeGrant {
        code,
        verifier,
        nonce,
        redirect_uri,
    })
}

async fn wait_callback(
    listener: tokio::net::TcpListener,
    expected_host: &str,
    expected_state: &str,
) -> Result<Zeroizing<String>> {
    for _ in 0..8 {
        let (mut stream, peer) = listener.accept().await?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let mut buf = vec![0u8; MAX_CALLBACK_HEAD];
        let mut used = 0usize;
        loop {
            if used == buf.len() {
                return Err(Error::Limit("OIDC/OAuth callback headers exceed 16 KiB"));
            }
            let n = stream.read(&mut buf[used..]).await?;
            if n == 0 {
                break;
            }
            used += n;
            if buf[..used].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let head = std::str::from_utf8(&buf[..used])
            .map_err(|_| Error::Protocol("invalid callback HTTP request"))?;
        let mut lines = head.split("\r\n");
        let first = lines
            .next()
            .ok_or(Error::Protocol("missing callback request line"))?;
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("");
        if method != "GET" || !target.starts_with('/') {
            continue;
        }
        let mut host = None;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("host") {
                    if host.is_some() {
                        return Err(Error::Denied("duplicate callback Host header"));
                    }
                    host = Some(value.trim());
                }
            }
        }
        if host != Some(expected_host) {
            continue;
        }
        let url = Url::parse(&format!("http://{expected_host}{target}"))
            .map_err(|_| Error::Protocol("invalid callback URL"))?;
        if url.path() != "/callback" {
            continue;
        }
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in url.query_pairs() {
            if values.insert(k.into_owned(), v.into_owned()).is_some() {
                return Err(Error::Denied("duplicate callback parameter"));
            }
        }
        if values.get("state").map(String::as_str) != Some(expected_state) {
            let _ = respond(&mut stream, 400, "State mismatch; return to the CLI.").await;
            return Err(Error::Denied("OIDC/OAuth state verification failed"));
        }
        if values.contains_key("error") {
            let _ = respond(
                &mut stream,
                400,
                "Authorization was denied; return to the CLI.",
            )
            .await;
            return Err(Error::Denied(
                "OIDC/OAuth authorization endpoint returned an error",
            ));
        }
        let code = values.remove("code").ok_or(Error::Denied(
            "OIDC/OAuth callback missing authorization code",
        ))?;
        if code.is_empty() || code.len() > 8192 || code.chars().any(char::is_control) {
            return Err(Error::Denied("invalid OIDC/OAuth authorization code"));
        }
        let _ = respond(
            &mut stream,
            200,
            "Authentication complete. You may close this tab.",
        )
        .await;
        return Ok(Zeroizing::new(code));
    }
    Err(Error::Denied("OIDC/OAuth callback was not received"))
}

async fn respond(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let value = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(value.as_bytes()).await
}

fn encoded_basic(client_id: &str, secret: &str) -> String {
    let id: String = form_urlencoded::byte_serialize(client_id.as_bytes()).collect();
    let secret: String = form_urlencoded::byte_serialize(secret.as_bytes()).collect();
    STANDARD.encode(format!("{id}:{secret}"))
}

async fn token_request(
    http: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    client_auth_method: &str,
    client_secret: Option<&[u8]>,
    fields: &[(&str, &str)],
) -> Result<TokenResponse> {
    let endpoint = safe_endpoint(token_endpoint)?;
    let secret = client_secret
        .map(|v| {
            std::str::from_utf8(v)
                .map_err(|_| Error::Denied("OIDC/OAuth client secret is not UTF-8"))
        })
        .transpose()?;
    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (k, v) in fields {
            serializer.append_pair(k, v);
        }
        match client_auth_method {
            "none" => {
                serializer.append_pair("client_id", client_id);
            }
            "client_secret_post" => {
                serializer.append_pair("client_id", client_id);
                serializer.append_pair(
                    "client_secret",
                    secret.ok_or(Error::Denied("OIDC/OAuth client secret unavailable"))?,
                );
            }
            "client_secret_basic" => {}
            _ => {
                return Err(Error::Config(
                    "unsupported OIDC/OAuth client authentication method".into(),
                ))
            }
        }
        Zeroizing::new(serializer.finish())
    };
    let mut request = http
        .post(endpoint)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .body(body.as_bytes().to_vec());
    if client_auth_method == "client_secret_basic" {
        let value = encoded_basic(
            client_id,
            secret.ok_or(Error::Denied("OIDC/OAuth client secret unavailable"))?,
        );
        request = request.header(reqwest::header::AUTHORIZATION, format!("Basic {value}"));
    }
    let response = request
        .send()
        .await
        .map_err(|_| Error::Transport("OIDC/OAuth token exchange failed"))?;
    let bytes = bounded_bytes(response).await?;
    let token: TokenResponse = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Protocol("OIDC/OAuth token endpoint returned invalid JSON"))?;
    if token.error.is_some() {
        return Err(Error::Denied("OIDC/OAuth token endpoint returned an error"));
    }
    Ok(token)
}

fn now_seconds() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Storage("system clock before Unix epoch"))?
            .as_secs(),
    )
    .map_err(|_| Error::Limit("system time overflow"))
}
fn now_micros() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Storage("system clock before Unix epoch"))?
            .as_micros(),
    )
    .map_err(|_| Error::Limit("system time overflow"))
}

fn aud_contains(aud: &serde_json::Value, client_id: &str) -> Result<(bool, usize)> {
    match aud {
        serde_json::Value::String(v) => Ok((v == client_id, 1)),
        serde_json::Value::Array(values) => {
            let mut count = 0usize;
            let mut found = false;
            for value in values {
                let s = value
                    .as_str()
                    .ok_or(Error::Protocol("OIDC aud claim must contain strings"))?;
                count += 1;
                found |= s == client_id;
            }
            Ok((found, count))
        }
        _ => Err(Error::Protocol("OIDC aud claim has invalid shape")),
    }
}

fn verify_id_token(
    token: &str,
    provider: &OidcProviderInfo,
    discovery: &Discovery,
    jwks: &Jwks,
    expected_nonce: &str,
) -> Result<String> {
    if token.len() > MAX_JWT_BYTES {
        return Err(Error::Limit("OIDC ID token exceeds 64 KiB"));
    }
    let mut segments = token.split('.');
    let h = segments
        .next()
        .ok_or(Error::Protocol("invalid OIDC ID token"))?;
    let p = segments
        .next()
        .ok_or(Error::Protocol("invalid OIDC ID token"))?;
    let s = segments
        .next()
        .ok_or(Error::Protocol("invalid OIDC ID token"))?;
    if segments.next().is_some() {
        return Err(Error::Protocol("invalid OIDC ID token"));
    }
    let header_bytes = URL_SAFE_NO_PAD
        .decode(h)
        .map_err(|_| Error::Protocol("invalid OIDC JWT header encoding"))?;
    let claims_bytes = URL_SAFE_NO_PAD
        .decode(p)
        .map_err(|_| Error::Protocol("invalid OIDC JWT claims encoding"))?;
    let header: JwtHeader = serde_json::from_slice(&header_bytes)
        .map_err(|_| Error::Protocol("invalid OIDC JWT header"))?;
    let claims: IdClaims = serde_json::from_slice(&claims_bytes)
        .map_err(|_| Error::Protocol("invalid OIDC ID-token claims"))?;
    if let Some(typ) = header.typ.as_deref() {
        if !typ.eq_ignore_ascii_case("JWT") {
            return Err(Error::Denied("OIDC JWT typ is not JWT"));
        }
    }
    if !matches!(header.alg.as_str(), "RS256" | "ES256") {
        return Err(Error::Denied(
            "OIDC ID-token signing algorithm is not allowed",
        ));
    }
    if !discovery.id_token_signing_alg_values_supported.is_empty()
        && !discovery
            .id_token_signing_alg_values_supported
            .iter()
            .any(|v| v == &header.alg)
    {
        return Err(Error::Denied(
            "OIDC ID-token algorithm not advertised by discovery",
        ));
    }
    let matching: Vec<&Jwk> = jwks
        .keys
        .iter()
        .filter(|k| {
            header
                .kid
                .as_deref()
                .is_none_or(|kid| k.kid.as_deref() == Some(kid))
                && k.alg.as_deref().is_none_or(|a| a == header.alg)
                && k.use_.as_deref().is_none_or(|u| u == "sig")
        })
        .collect();
    if matching.len() != 1 {
        return Err(Error::Denied(
            "OIDC signing key selection is missing or ambiguous",
        ));
    }
    let signed = format!("{h}.{p}");
    let sig = URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| Error::Protocol("invalid OIDC JWT signature encoding"))?;
    let key = matching[0];
    match header.alg.as_str() {
        "RS256" => {
            if key.kty != "RSA" {
                return Err(Error::Denied("OIDC JWKS key type does not match RS256"));
            }
            let n = URL_SAFE_NO_PAD
                .decode(
                    key.n
                        .as_deref()
                        .ok_or(Error::Protocol("RSA JWK missing modulus"))?,
                )
                .map_err(|_| Error::Protocol("invalid RSA JWK modulus"))?;
            let e = URL_SAFE_NO_PAD
                .decode(
                    key.e
                        .as_deref()
                        .ok_or(Error::Protocol("RSA JWK missing exponent"))?,
                )
                .map_err(|_| Error::Protocol("invalid RSA JWK exponent"))?;
            signature::RsaPublicKeyComponents { n: &n, e: &e }
                .verify(
                    &signature::RSA_PKCS1_2048_8192_SHA256,
                    signed.as_bytes(),
                    &sig,
                )
                .map_err(|_| Error::Denied("OIDC ID-token signature verification failed"))?;
        }
        "ES256" => {
            if key.kty != "EC" || key.crv.as_deref() != Some("P-256") {
                return Err(Error::Denied("OIDC JWKS key type does not match ES256"));
            }
            let x = URL_SAFE_NO_PAD
                .decode(
                    key.x
                        .as_deref()
                        .ok_or(Error::Protocol("EC JWK missing x"))?,
                )
                .map_err(|_| Error::Protocol("invalid EC JWK x"))?;
            let y = URL_SAFE_NO_PAD
                .decode(
                    key.y
                        .as_deref()
                        .ok_or(Error::Protocol("EC JWK missing y"))?,
                )
                .map_err(|_| Error::Protocol("invalid EC JWK y"))?;
            if x.len() != 32 || y.len() != 32 {
                return Err(Error::Protocol("invalid P-256 JWK coordinate length"));
            }
            let mut public = Vec::with_capacity(65);
            public.push(4);
            public.extend_from_slice(&x);
            public.extend_from_slice(&y);
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &public)
                .verify(signed.as_bytes(), &sig)
                .map_err(|_| Error::Denied("OIDC ID-token signature verification failed"))?;
        }
        _ => unreachable!(),
    }
    if claims.iss != provider.issuer || claims.iss != discovery.issuer {
        return Err(Error::Denied("OIDC ID-token issuer mismatch"));
    }
    if claims.nonce != expected_nonce {
        return Err(Error::Denied("OIDC ID-token nonce mismatch"));
    }
    if claims.sub.trim().is_empty()
        || claims.sub.len() > 512
        || claims.sub.chars().any(char::is_control)
    {
        return Err(Error::Denied("OIDC subject claim is invalid"));
    }
    let (aud_ok, aud_count) = aud_contains(&claims.aud, &provider.client_id)?;
    if !aud_ok {
        return Err(Error::Denied("OIDC ID-token audience mismatch"));
    }
    if aud_count > 1 && claims.azp.as_deref() != Some(provider.client_id.as_str()) {
        return Err(Error::Denied("OIDC ID-token authorized party mismatch"));
    }
    if claims
        .azp
        .as_deref()
        .is_some_and(|v| v != provider.client_id)
    {
        return Err(Error::Denied("OIDC ID-token authorized party mismatch"));
    }
    let now = now_seconds()?;
    if claims.exp <= now - CLOCK_SKEW_SECONDS {
        return Err(Error::Denied("OIDC ID token is expired"));
    }
    if claims.iat > now + CLOCK_SKEW_SECONDS
        || claims.nbf.is_some_and(|nbf| nbf > now + CLOCK_SKEW_SECONDS)
    {
        return Err(Error::Denied(
            "OIDC ID-token time claims are not currently valid",
        ));
    }
    Ok(claims.sub)
}

async fn verified_oidc_flow(
    store: &SessionStore,
    provider_id: &str,
    target_user_id: Option<&str>,
    listen: SocketAddr,
    timeout: Duration,
    on_url: impl FnOnce(&str),
) -> Result<VerifiedOidcLogin> {
    let provider = store.oidc_provider_for_login(provider_id)?;
    let http = client()?;
    let discovery = discovery(&http, &provider).await?;
    let grant = prepare_code_flow(
        &discovery.authorization_endpoint,
        &provider.client_id,
        &provider.scopes,
        listen,
        timeout,
        true,
        on_url,
    )
    .await?;
    let secrets = store.local_secret_store()?;
    let client_secret = store.resolve_oidc_client_secret(provider_id, &secrets)?;
    let token = token_request(
        &http,
        &discovery.token_endpoint,
        &provider.client_id,
        &provider.client_auth_method,
        client_secret.as_deref().map(Vec::as_slice),
        &[
            ("grant_type", "authorization_code"),
            ("code", grant.code.as_str()),
            ("redirect_uri", &grant.redirect_uri),
            ("code_verifier", grant.verifier.as_str()),
        ],
    )
    .await?;
    let id_token = token
        .id_token
        .as_deref()
        .ok_or(Error::Denied("OIDC token response omitted ID token"))?;
    let jwks: Jwks = fetch_json(&http, &discovery.jwks_uri).await?;
    if jwks.keys.is_empty() || jwks.keys.len() > 128 {
        return Err(Error::Limit("OIDC JWKS key count outside bound"));
    }
    let subject = verify_id_token(
        id_token,
        &provider,
        &discovery,
        &jwks,
        grant
            .nonce
            .as_ref()
            .ok_or(Error::Protocol("OIDC nonce missing"))?,
    )?;
    store.complete_verified_oidc(provider_id, &provider.issuer, &subject, target_user_id)
}

pub async fn oidc_link(
    store: &SessionStore,
    provider_id: &str,
    target_user_id: &str,
    listen: SocketAddr,
    timeout: Duration,
    on_url: impl FnOnce(&str),
) -> Result<VerifiedOidcLogin> {
    verified_oidc_flow(
        store,
        provider_id,
        Some(target_user_id),
        listen,
        timeout,
        on_url,
    )
    .await
}

pub async fn oidc_login(
    store: &SessionStore,
    provider_id: &str,
    ttl_seconds: u64,
    listen: SocketAddr,
    timeout: Duration,
    on_url: impl FnOnce(&str),
) -> Result<OidcFlowIssued> {
    let login = verified_oidc_flow(store, provider_id, None, listen, timeout, on_url).await?;
    let issued = store.issue_auth_session_from_verified_oidc(
        &login.identity.identity_id,
        ttl_seconds,
        serde_json::json!({"client":"oidc-loopback","provider_id":provider_id}),
    )?;
    Ok(OidcFlowIssued {
        login,
        session: issued.info,
        refresh_token: issued.refresh_token,
    })
}

struct ChatGptCallback {
    code: Zeroizing<String>,
    client_id: String,
}

async fn wait_chatgpt_callback(
    listener: tokio::net::TcpListener,
    expected_host: &str,
    expected_state: &str,
) -> Result<ChatGptCallback> {
    for _ in 0..8 {
        let (mut stream, peer) = listener.accept().await?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let mut buf = vec![0u8; MAX_CALLBACK_HEAD];
        let mut used = 0usize;
        loop {
            if used == buf.len() {
                return Err(Error::Limit(
                    "Sign in with ChatGPT callback headers exceed 16 KiB",
                ));
            }
            let n = stream.read(&mut buf[used..]).await?;
            if n == 0 {
                break;
            }
            used += n;
            if buf[..used].windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let head = std::str::from_utf8(&buf[..used])
            .map_err(|_| Error::Protocol("invalid Sign in with ChatGPT callback"))?;
        let mut lines = head.split("\r\n");
        let first = lines
            .next()
            .ok_or(Error::Protocol("missing callback request line"))?;
        let mut parts = first.split_whitespace();
        if parts.next() != Some("GET") {
            continue;
        }
        let target = parts.next().unwrap_or("");
        if !target.starts_with('/') {
            continue;
        }
        let mut host = None;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("host") {
                    if host.is_some() {
                        return Err(Error::Denied("duplicate callback Host header"));
                    }
                    host = Some(value.trim());
                }
            }
        }
        if host != Some(expected_host) {
            continue;
        }
        let url = Url::parse(&format!("http://{expected_host}{target}"))
            .map_err(|_| Error::Protocol("invalid callback URL"))?;
        if url.path() != "/auth/callback" {
            continue;
        }
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if values
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(Error::Denied("duplicate callback parameter"));
            }
        }
        if values.get("state").map(String::as_str) != Some(expected_state) {
            let _ = respond(&mut stream, 400, "State mismatch; return to aw.").await;
            return Err(Error::Denied(
                "Sign in with ChatGPT state verification failed",
            ));
        }
        if values.contains_key("error") {
            let _ = respond(&mut stream, 400, "Authorization was denied; return to aw.").await;
            return Err(Error::Denied(
                "Sign in with ChatGPT authorization was not granted",
            ));
        }
        let code = values
            .remove("code")
            .ok_or(Error::Denied("ChatGPT callback missing authorization code"))?;
        let client_id = values.remove("client_id").ok_or(Error::Denied(
            "ChatGPT registration did not return an issued client ID",
        ))?;
        if code.is_empty()
            || code.len() > 8192
            || code.chars().any(char::is_control)
            || client_id.is_empty()
            || client_id.len() > 512
            || client_id.chars().any(char::is_control)
            || client_id == CHATGPT_DYNAMIC_CLIENT
        {
            return Err(Error::Denied("invalid ChatGPT registration callback"));
        }
        let _ = respond(
            &mut stream,
            200,
            "ChatGPT authorization complete. You may close this tab.",
        )
        .await;
        return Ok(ChatGptCallback {
            code: Zeroizing::new(code),
            client_id,
        });
    }
    Err(Error::Denied(
        "Sign in with ChatGPT callback was not received",
    ))
}

fn chatgpt_scopes() -> BTreeSet<String> {
    CHATGPT_SCOPE
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

pub async fn chatgpt_plan_authorize(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
    host_id: &str,
    listen: SocketAddr,
    timeout: Duration,
    on_url: impl FnOnce(&str),
) -> Result<OAuthConnectorInfo> {
    if listen.ip() != IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        return Err(Error::Denied(
            "Sign in with ChatGPT callback must bind 127.0.0.1",
        ));
    }
    if !host_id.starts_with("urn:uuid:") || host_id.len() != 45 {
        return Err(Error::Config(
            "ChatGPT host ID must be a persisted urn:uuid:<uuid>".into(),
        ));
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let local = listener.local_addr()?;
    let redirect_uri = format!("http://127.0.0.1:{}/auth/callback", local.port());
    let state = random_b64(32)?;
    let verifier = random_b64(32)?;
    let nonce = random_b64(32)?;
    let mut url = safe_endpoint(CHATGPT_AUTHORIZE)?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", CHATGPT_DYNAMIC_CLIENT)
            .append_pair("agent_name_hint", "aw")
            .append_pair("ext_agent_host_id", host_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", CHATGPT_SCOPE)
            .append_pair("resource", CHATGPT_RESOURCE)
            .append_pair("state", state.as_str())
            .append_pair("nonce", nonce.as_str())
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &pkce_challenge(&verifier));
    }
    on_url(url.as_str());
    let callback = tokio::time::timeout(
        timeout,
        wait_chatgpt_callback(listener, &authority(local), state.as_str()),
    )
    .await
    .map_err(|_| Error::Timeout)??;

    let http = client()?;
    let token = token_request(
        &http,
        CHATGPT_TOKEN,
        &callback.client_id,
        "none",
        None,
        &[
            ("grant_type", "authorization_code"),
            ("code", callback.code.as_str()),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", verifier.as_str()),
            ("resource", CHATGPT_RESOURCE),
        ],
    )
    .await?;

    if token
        .token_type
        .as_deref()
        .is_none_or(|value| !value.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::Denied(
            "ChatGPT OAuth token response is not bearer-token based",
        ));
    }
    let requested = chatgpt_scopes();
    let granted = effective_scopes(token.scope.as_deref(), &requested)?;
    if !granted.contains("chatgpt.tokens.use.direct") {
        return Err(Error::Denied(
            "ChatGPT plan usage was not authorized for this app",
        ));
    }
    let id_token = token.id_token.as_deref().ok_or(Error::Denied(
        "ChatGPT OAuth token response omitted ID token",
    ))?;

    let discovery_url = format!("{CHATGPT_ISSUER}/.well-known/openid-configuration");
    let discovery: Discovery = fetch_json(&http, &discovery_url).await?;
    if discovery.issuer != CHATGPT_ISSUER {
        return Err(Error::Denied("ChatGPT OIDC issuer mismatch"));
    }
    let jwks: Jwks = fetch_json(&http, &discovery.jwks_uri).await?;
    if jwks.keys.is_empty() || jwks.keys.len() > 128 {
        return Err(Error::Limit("ChatGPT JWKS key count outside bound"));
    }
    let provider = OidcProviderInfo {
        provider_id: connector_id.to_owned(),
        issuer: CHATGPT_ISSUER.into(),
        client_id: callback.client_id.clone(),
        client_auth_method: "none".into(),
        scopes: requested.clone(),
        status: "active".into(),
        client_secret_version: None,
        created_by_user_id: actor.as_str().to_owned(),
        created_at: 0,
        rotated_at: None,
        revoked_at: None,
    };
    let subject = verify_id_token(id_token, &provider, &discovery, &jwks, nonce.as_str())?;

    let secrets = store.local_secret_store()?;
    let connector = store.create_oauth_connector(
        actor,
        &secrets,
        connector_id,
        CHATGPT_AUTHORIZE,
        CHATGPT_TOKEN,
        &callback.client_id,
        "none",
        requested,
        None,
    )?;
    let access = token.access_token.as_deref().ok_or(Error::Denied(
        "ChatGPT OAuth token response omitted access token",
    ))?;
    let expires = token
        .expires_in
        .map(|seconds| {
            now_micros().and_then(|now| {
                now.checked_add(
                    i64::try_from(seconds)
                        .map_err(|_| Error::Limit("OAuth expiry overflow"))?
                        .checked_mul(1_000_000)
                        .ok_or(Error::Limit("OAuth expiry overflow"))?,
                )
                .ok_or(Error::Limit("OAuth expiry overflow"))
            })
        })
        .transpose()?;
    let set = OAuthTokenSet {
        access_token: access.to_owned(),
        refresh_token: token.refresh_token.clone(),
        id_token: token.id_token.clone(),
        oidc_issuer: Some(CHATGPT_ISSUER.into()),
        oidc_subject: Some(subject),
        token_type: "Bearer".into(),
        scopes: granted,
        access_expires_at: expires,
    };
    store.store_oauth_token_set(actor, &secrets, connector_id, &set)?;
    Ok(connector)
}

async fn chatgpt_refresh_if_needed(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
) -> Result<OAuthTokenSet> {
    let secrets = store.local_secret_store()?;
    let mut token = store.load_oauth_token_set(actor, &secrets, connector_id)?;
    let now = now_micros()?;
    if token
        .access_expires_at
        .is_none_or(|expiry| expiry > now.saturating_add(120_000_000))
    {
        return Ok(token);
    }
    let refresh = token
        .refresh_token
        .as_deref()
        .ok_or(Error::Denied("ChatGPT connection needs sign-in again"))?;
    let connector = store.oauth_connector_for_owner(actor, connector_id)?;
    let http = client()?;
    let next = token_request(
        &http,
        CHATGPT_TOKEN,
        &connector.client_id,
        "none",
        None,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("resource", CHATGPT_RESOURCE),
        ],
    )
    .await?;
    if next
        .token_type
        .as_deref()
        .is_none_or(|value| !value.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::Denied(
            "ChatGPT OAuth refresh did not return a bearer token",
        ));
    }
    let requested = chatgpt_scopes();
    let granted = effective_scopes(next.scope.as_deref(), &requested)?;
    if !granted.contains("chatgpt.tokens.use.direct") {
        return Err(Error::Denied(
            "refreshed ChatGPT token no longer grants plan usage",
        ));
    }
    let access = next
        .access_token
        .as_deref()
        .ok_or(Error::Denied("ChatGPT OAuth refresh omitted access token"))?;
    let expires = next
        .expires_in
        .map(|seconds| {
            now_micros().and_then(|current| {
                current
                    .checked_add(
                        i64::try_from(seconds)
                            .map_err(|_| Error::Limit("OAuth expiry overflow"))?
                            .checked_mul(1_000_000)
                            .ok_or(Error::Limit("OAuth expiry overflow"))?,
                    )
                    .ok_or(Error::Limit("OAuth expiry overflow"))
            })
        })
        .transpose()?;
    let replacement = OAuthTokenSet {
        access_token: access.to_owned(),
        refresh_token: next
            .refresh_token
            .clone()
            .or_else(|| token.refresh_token.take()),
        id_token: next.id_token.clone().or_else(|| token.id_token.take()),
        oidc_issuer: token.oidc_issuer.take(),
        oidc_subject: token.oidc_subject.take(),
        token_type: "Bearer".into(),
        scopes: granted,
        access_expires_at: expires,
    };
    store.store_oauth_token_set(actor, &secrets, connector_id, &replacement)?;
    store.load_oauth_token_set(actor, &secrets, connector_id)
}

pub async fn chatgpt_plan_access_token(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
) -> Result<Zeroizing<String>> {
    let token = chatgpt_refresh_if_needed(store, actor, connector_id).await?;
    if !token.scopes.contains("chatgpt.tokens.use.direct") {
        return Err(Error::Denied(
            "selected ChatGPT account is not authorized for plan usage",
        ));
    }
    if token.oidc_issuer.as_deref() != Some(CHATGPT_ISSUER)
        || token
            .oidc_subject
            .as_deref()
            .is_none_or(|subject| subject.is_empty())
    {
        return Err(Error::Denied(
            "selected ChatGPT connection lacks its exact verified issuer+subject identity",
        ));
    }
    Ok(Zeroizing::new(token.access_token.clone()))
}

async fn chatgpt_remote_revoke(
    http: &reqwest::Client,
    connector: &OAuthConnectorInfo,
    refresh_token: &str,
) -> Result<()> {
    let discovery_url = format!("{CHATGPT_ISSUER}/.well-known/openid-configuration");
    let discovery: Discovery = fetch_json(http, &discovery_url).await?;
    if discovery.issuer != CHATGPT_ISSUER {
        return Err(Error::Denied("ChatGPT OIDC issuer mismatch"));
    }
    let endpoint = discovery
        .revocation_endpoint
        .as_deref()
        .ok_or(Error::Protocol(
            "ChatGPT discovery omitted revocation endpoint",
        ))?;
    let endpoint = safe_endpoint(endpoint)?;
    if endpoint.scheme() != "https"
        || endpoint.host_str() != Some("auth.openai.com")
        || endpoint.query().is_some()
    {
        return Err(Error::Denied(
            "ChatGPT revocation endpoint is outside the trusted issuer",
        ));
    }
    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("token", refresh_token)
            .append_pair("token_type_hint", "refresh_token")
            .append_pair("client_id", &connector.client_id);
        Zeroizing::new(serializer.finish())
    };
    let response = http
        .post(endpoint)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body.as_str().to_owned())
        .send()
        .await
        .map_err(|_| Error::Transport("ChatGPT OAuth revocation request failed"))?;
    let _ = bounded_bytes(response).await?;
    Ok(())
}

/// End the renewable ChatGPT OAuth session when possible, then remove the
/// local SecretStore credential and connector metadata. A false return means
/// local sign-out completed but remote revocation could not be confirmed.
pub async fn chatgpt_plan_disconnect(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
) -> Result<bool> {
    let secrets = store.local_secret_store()?;
    let connector = store.oauth_connector_for_owner(actor, connector_id)?;
    let token = store.load_oauth_token_set(actor, &secrets, connector_id)?;
    let remote_confirmed = match token.refresh_token.as_deref() {
        Some(refresh) => match client() {
            Ok(http) => chatgpt_remote_revoke(&http, &connector, refresh)
                .await
                .is_ok(),
            Err(_) => false,
        },
        None => false,
    };
    store.revoke_oauth_connector(actor, &secrets, connector_id)?;
    Ok(remote_confirmed)
}

/// Clean up abandoned Sign in with ChatGPT connectors only when the user is
/// already starting another explicit ChatGPT setup. This avoids provider
/// contact during ordinary startup/settings reads while ensuring an interrupted
/// setup is not retained forever.
pub async fn chatgpt_plan_cleanup_unreferenced(
    store: &SessionStore,
    actor: &PrincipalId,
    referenced: &BTreeSet<String>,
) -> Result<usize> {
    let mut cleaned = 0usize;
    for connector in store.oauth_connectors(actor)? {
        if referenced.contains(&connector.connector_id)
            || connector.status == "revoked"
            || connector.authorization_endpoint != CHATGPT_AUTHORIZE
            || connector.token_endpoint != CHATGPT_TOKEN
            || !connector
                .requested_scopes
                .contains("chatgpt.tokens.use.direct")
        {
            continue;
        }
        let _ = chatgpt_plan_disconnect(store, actor, &connector.connector_id).await?;
        cleaned = cleaned.saturating_add(1);
    }
    Ok(cleaned)
}

pub async fn chatgpt_plan_models(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
) -> Result<Vec<ChatGptPlanModel>> {
    #[derive(Deserialize)]
    struct Catalog {
        models: Vec<CatalogModel>,
    }
    #[derive(Deserialize)]
    struct CatalogModel {
        slug: String,
        display_name: String,
        visibility: String,
    }

    let token = chatgpt_plan_access_token(store, actor, connector_id).await?;
    let http = client()?;
    let response = http
        .get("https://api.openai.com/v1/models")
        .bearer_auth(token.as_str())
        .send()
        .await
        .map_err(|_| Error::Transport("ChatGPT model-list request failed"))?;
    let body = bounded_bytes(response).await?;
    let catalog: Catalog = serde_json::from_slice(&body)
        .map_err(|_| Error::Protocol("ChatGPT model-list response was invalid"))?;
    let mut out = Vec::new();
    for model in catalog
        .models
        .into_iter()
        .filter(|model| model.visibility == "list")
    {
        if model.slug.trim().is_empty()
            || model.slug.len() > 256
            || model.display_name.trim().is_empty()
            || model.display_name.len() > 256
        {
            continue;
        }
        out.push(ChatGptPlanModel {
            slug: model.slug,
            display_name: model.display_name,
        });
        if out.len() >= 128 {
            break;
        }
    }
    if out.is_empty() {
        return Err(Error::Denied(
            "ChatGPT account returned no selectable models",
        ));
    }
    Ok(out)
}

pub async fn oauth_authorize(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
    listen: SocketAddr,
    timeout: Duration,
    on_url: impl FnOnce(&str),
) -> Result<OAuthConnectorInfo> {
    let connector = store.oauth_connector_for_owner(actor, connector_id)?;
    let grant = prepare_code_flow(
        &connector.authorization_endpoint,
        &connector.client_id,
        &connector.requested_scopes,
        listen,
        timeout,
        false,
        on_url,
    )
    .await?;
    let secrets = store.local_secret_store()?;
    let client_secret = store.resolve_oauth_client_secret(actor, connector_id, &secrets)?;
    let http = client()?;
    let token = token_request(
        &http,
        &connector.token_endpoint,
        &connector.client_id,
        &connector.client_auth_method,
        client_secret.as_deref().map(Vec::as_slice),
        &[
            ("grant_type", "authorization_code"),
            ("code", grant.code.as_str()),
            ("redirect_uri", &grant.redirect_uri),
            ("code_verifier", grant.verifier.as_str()),
        ],
    )
    .await?;
    let access = token
        .access_token
        .as_deref()
        .ok_or(Error::Denied("OAuth token response omitted access token"))?;
    if token
        .token_type
        .as_deref()
        .is_none_or(|v| !v.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::Denied(
            "OAuth token response is not bearer-token based",
        ));
    }
    let scopes = effective_scopes(token.scope.as_deref(), &connector.requested_scopes)?;
    let expires = token
        .expires_in
        .map(|v| {
            now_micros().and_then(|n| {
                n.checked_add(
                    i64::try_from(v)
                        .map_err(|_| Error::Limit("OAuth expiry overflow"))?
                        .checked_mul(1_000_000)
                        .ok_or(Error::Limit("OAuth expiry overflow"))?,
                )
                .ok_or(Error::Limit("OAuth expiry overflow"))
            })
        })
        .transpose()?;
    let set = OAuthTokenSet {
        access_token: access.to_owned(),
        refresh_token: token.refresh_token.clone(),
        id_token: None,
        oidc_issuer: None,
        oidc_subject: None,
        token_type: "Bearer".into(),
        scopes,
        access_expires_at: expires,
    };
    store.store_oauth_token_set(actor, &secrets, connector_id, &set)
}

pub async fn oauth_refresh(
    store: &SessionStore,
    actor: &PrincipalId,
    connector_id: &str,
) -> Result<OAuthConnectorInfo> {
    let connector = store.oauth_connector_for_owner(actor, connector_id)?;
    let secrets = store.local_secret_store()?;
    let old = store.load_oauth_token_set(actor, &secrets, connector_id)?;
    let refresh = old
        .refresh_token
        .as_deref()
        .ok_or(Error::Denied("OAuth connector has no refresh token"))?;
    let client_secret = store.resolve_oauth_client_secret(actor, connector_id, &secrets)?;
    let http = client()?;
    let token = token_request(
        &http,
        &connector.token_endpoint,
        &connector.client_id,
        &connector.client_auth_method,
        client_secret.as_deref().map(Vec::as_slice),
        &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
    )
    .await?;
    let access = token
        .access_token
        .as_deref()
        .ok_or(Error::Denied("OAuth refresh response omitted access token"))?;
    if token
        .token_type
        .as_deref()
        .is_none_or(|v| !v.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::Denied(
            "OAuth refresh response is not bearer-token based",
        ));
    }
    let scopes = effective_scopes(token.scope.as_deref(), &old.scopes)?;
    let expires = token
        .expires_in
        .map(|v| {
            now_micros().and_then(|n| {
                n.checked_add(
                    i64::try_from(v)
                        .map_err(|_| Error::Limit("OAuth expiry overflow"))?
                        .checked_mul(1_000_000)
                        .ok_or(Error::Limit("OAuth expiry overflow"))?,
                )
                .ok_or(Error::Limit("OAuth expiry overflow"))
            })
        })
        .transpose()?;
    let set = OAuthTokenSet {
        access_token: access.to_owned(),
        refresh_token: token
            .refresh_token
            .clone()
            .or_else(|| old.refresh_token.clone()),
        id_token: None,
        oidc_issuer: None,
        oidc_subject: None,
        token_type: "Bearer".into(),
        scopes,
        access_expires_at: expires,
    };
    store.store_oauth_token_set(actor, &secrets, connector_id, &set)
}

fn effective_scopes(value: Option<&str>, requested: &BTreeSet<String>) -> Result<BTreeSet<String>> {
    let result = match value {
        Some(v) => v.split_whitespace().map(str::to_owned).collect(),
        None => requested.clone(),
    };
    if result.is_empty() || !result.is_subset(requested) {
        return Err(Error::Denied(
            "OAuth token scope exceeds or omits requested connector scope",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audience_shapes() {
        let (ok, n) = aud_contains(&serde_json::json!(["a", "b"]), "b").unwrap();
        assert!(ok);
        assert_eq!(n, 2);
        assert!(!aud_contains(&serde_json::json!("a"), "b").unwrap().0);
    }
    #[test]
    fn endpoint_policy() {
        assert!(safe_endpoint("https://example.com/oauth").is_ok());
        assert!(safe_endpoint("http://127.0.0.1:1234/oauth").is_ok());
        assert!(safe_endpoint("http://example.com/oauth").is_err());
        assert!(safe_endpoint("https://u:p@example.com/oauth").is_err());
    }
    #[test]
    fn pkce_is_stable() {
        assert_eq!(
            pkce_challenge("abc"),
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"abc"))
        );
    }
}
