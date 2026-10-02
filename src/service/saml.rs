// Copyright 2026 OpenObserve Inc.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Open-source, standard SAML 2.0 Service Provider (SP) support.
//!
//! Implements an SP-initiated SSO flow against a SAML 2.0 IdP (e.g. Tencent
//! Taihu). It is self-contained and does NOT depend on the closed-source
//! enterprise crates.
//!
//! Flow:
//!   1. [`sp_metadata`]    — SP metadata XML to register this SP at the IdP.
//!   2. [`login_redirect_url`] — build the AuthnRequest and the HTTP-Redirect
//!      URL the browser is sent to (DEFLATE + base64 + urlencode).
//!   3. IdP authenticates the user and HTTP-POSTs a `SAMLResponse` back to the
//!      SP ACS endpoint.
//!   4. [`verify_and_extract`] — base64-decode, verify the assertion signature
//!      (`WantAssertionsSigned=true`) against the configured IdP certificate,
//!      and extract the user's email / name attributes.
//!
//! Security note: the assertion signature is verified using exclusive XML
//! canonicalization (`http://www.w3.org/2001/10/xml-exc-c14n#`) + RSA PKCS#1
//! v1.5 (SHA-1 or SHA-256, per the `SignatureMethod`). XML canonicalization is
//! subtle; the implementation targets the standard SAML assertion structure.
//! Per the Taihu integration notes, validate once against a real IdP response.

use config::get_config;

/// SAML XML namespaces.
const NS_SAMLP: &str = "urn:oasis:names:tc:SAML:2.0:protocol";
const NS_SAML: &str = "urn:oasis:names:tc:SAML:2.0:assertion";
const NS_DS: &str = "http://www.w3.org/2000/09/xmldsig#";
const NS_METADATA: &str = "urn:oasis:names:tc:SAML:2.0:metadata";

const BINDING_POST: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST";
const NAMEID_TRANSIENT: &str = "urn:oasis:names:tc:SAML:2.0:nameid-format:transient";

/// Parsed, verified SAML assertion: the data we actually consume.
#[derive(Debug, Clone, Default)]
pub struct SamlAssertion {
    /// User principal / email (from `saml_attr_email`, falling back to NameID).
    pub email: String,
    /// Optional display name (from `saml_attr_name`).
    pub name: String,
}

// ---------------------------------------------------------------------------
// 1. SP metadata
// ---------------------------------------------------------------------------

/// Build the SP metadata XML to register this SP at the IdP.
///
/// `AuthnRequestsSigned=false` (the SP does not sign its requests) and
/// `WantAssertionsSigned=true` (the IdP must sign assertions — the core
/// security property). No encryption KeyDescriptor is declared.
pub fn sp_metadata() -> Result<String, anyhow::Error> {
    let cfg = get_config();
    let sp_entity = cfg.auth.saml_sp_entity_id.trim();
    let acs = cfg.auth.saml_sp_acs_url.trim();
    if sp_entity.is_empty() || acs.is_empty() {
        return Err(anyhow::anyhow!(
            "ZO_SAML_SP_ENTITY_ID / ZO_SAML_SP_ACS_URL are not set"
        ));
    }
    let xml = format!(
        r#"<?xml version="1.0"?>
<md:EntityDescriptor xmlns:md="{NS_METADATA}" entityID="{sp_entity}">
  <md:SPSSODescriptor AuthnRequestsSigned="false" WantAssertionsSigned="true" protocolSupportEnumeration="{NS_SAMLP}">
    <md:NameIDFormat>{NAMEID_TRANSIENT}</md:NameIDFormat>
    <md:AssertionConsumerService Binding="{BINDING_POST}" Location="{acs}" index="1" isDefault="true"/>
  </md:SPSSODescriptor>
</md:EntityDescriptor>"#,
        sp_entity = xml_escape_attr(sp_entity),
        acs = xml_escape_attr(acs),
    );
    Ok(xml)
}

// ---------------------------------------------------------------------------
// 2. AuthnRequest + HTTP-Redirect URL
// ---------------------------------------------------------------------------

/// Build the AuthnRequest and the full HTTP-Redirect URL to the IdP SSO
/// endpoint. `relay_state` is echoed back by the IdP on the ACS callback.
pub fn login_redirect_url(relay_state: &str) -> Result<String, anyhow::Error> {
    use std::io::Write;

    use config::utils::rand::generate_random_string;
    use flate2::{Compression, write::DeflateEncoder};

    let cfg = get_config();
    let sso_url = cfg.auth.saml_idp_sso_url.trim();
    let sp_entity = cfg.auth.saml_sp_entity_id.trim();
    let acs = cfg.auth.saml_sp_acs_url.trim();
    if sso_url.is_empty() || sp_entity.is_empty() || acs.is_empty() {
        return Err(anyhow::anyhow!(
            "SAML IdP SSO URL / SP entityID / ACS URL are not set"
        ));
    }

    // AuthnRequest ID must be an XML NCName: start with a letter/underscore.
    let id = format!("_{}", generate_random_string(32));
    let issue_instant = utc_now_iso8601();

    let authn_request = format!(
        r#"<samlp:AuthnRequest xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" ID="{id}" Version="2.0" IssueInstant="{issue_instant}" Destination="{dest}" ProtocolBinding="{BINDING_POST}" AssertionConsumerServiceURL="{acs}"><saml:Issuer>{issuer}</saml:Issuer><samlp:NameIDPolicy Format="{NAMEID_TRANSIENT}" AllowCreate="true"/></samlp:AuthnRequest>"#,
        id = id,
        issue_instant = issue_instant,
        dest = xml_escape_attr(sso_url),
        acs = xml_escape_attr(acs),
        issuer = xml_escape_text(sp_entity),
    );

    // HTTP-Redirect binding: raw DEFLATE (RFC 1951) -> base64 -> urlencode.
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(authn_request.as_bytes())?;
    let deflated = encoder.finish()?;
    let b64 = b64_encode_bytes(&deflated);

    let mut url = url::Url::parse(sso_url)
        .map_err(|e| anyhow::anyhow!("invalid ZO_SAML_IDP_SSO_URL: {e}"))?;
    url.query_pairs_mut()
        .append_pair("SAMLRequest", &b64)
        .append_pair("RelayState", relay_state);
    Ok(url.to_string())
}

// ---------------------------------------------------------------------------
// 3. Verify SAMLResponse + extract attributes
// ---------------------------------------------------------------------------

/// Verify a base64-encoded `SAMLResponse` (as POSTed to the ACS endpoint) and
/// extract the user identity.
///
/// Steps: base64 decode -> parse XML -> locate the signed `Assertion` ->
/// verify its signature against the configured IdP certificate -> check status
/// is Success -> extract email/name attributes.
pub fn verify_and_extract(saml_response_b64: &str) -> Result<SamlAssertion, anyhow::Error> {
    let cfg = get_config();

    let xml_bytes = config::utils::base64::decode_raw(saml_response_b64)
        .map_err(|e| anyhow::anyhow!("SAMLResponse is not valid base64: {e}"))?;
    let xml = String::from_utf8(xml_bytes)
        .map_err(|e| anyhow::anyhow!("SAMLResponse is not valid UTF-8: {e}"))?;

    let doc = roxmltree::Document::parse(&xml)
        .map_err(|e| anyhow::anyhow!("SAMLResponse is not valid XML: {e}"))?;
    let root = doc.root_element();

    // Must be a samlp:Response.
    if root.tag_name().name() != "Response" || root.tag_name().namespace() != Some(NS_SAMLP) {
        return Err(anyhow::anyhow!("not a SAML Response"));
    }

    // Status must be Success.
    let status_ok = root
        .descendants()
        .find(|n| n.tag_name().name() == "StatusCode" && n.tag_name().namespace() == Some(NS_SAMLP))
        .and_then(|n| n.attribute("Value"))
        .map(|v| v.ends_with(":status:Success"))
        .unwrap_or(false);
    if !status_ok {
        return Err(anyhow::anyhow!("SAML Response status is not Success"));
    }

    // Locate the Assertion element.
    let assertion = root
        .descendants()
        .find(|n| n.tag_name().name() == "Assertion" && n.tag_name().namespace() == Some(NS_SAML))
        .ok_or_else(|| anyhow::anyhow!("SAML Response has no Assertion"))?;

    // Verify the assertion signature (WantAssertionsSigned=true).
    verify_assertion_signature(&xml, &doc, assertion)?;

    // Extract identity.
    let email = extract_attr_value(assertion, &cfg.auth.saml_attr_email)
        .or_else(|| {
            // Fallback: NameID (nominally transient; last resort only).
            assertion
                .descendants()
                .find(|n| n.tag_name().name() == "NameID" && n.tag_name().namespace() == Some(NS_SAML))
                .and_then(|n| n.text())
                .map(|s| s.to_string())
        })
        .ok_or_else(|| anyhow::anyhow!("assertion has no email attribute or NameID"))?;

    let name = extract_attr_value(assertion, &cfg.auth.saml_attr_name).unwrap_or_default();

    Ok(SamlAssertion {
        email: email.trim().to_string(),
        name: name.trim().to_string(),
    })
}

/// Read the first value of a SAML `AttributeStatement/Attribute` whose `Name`
/// matches `attr_name`.
fn extract_attr_value(assertion: roxmltree::Node, attr_name: &str) -> Option<String> {
    assertion
        .descendants()
        .filter(|n| n.tag_name().name() == "Attribute" && n.tag_name().namespace() == Some(NS_SAML))
        .find(|n| n.attribute("Name") == Some(attr_name))
        .and_then(|attr| {
            attr.descendants()
                .find(|n| {
                    n.tag_name().name() == "AttributeValue"
                        && n.tag_name().namespace() == Some(NS_SAML)
                })
                .and_then(|v| v.text())
                .map(|s| s.to_string())
        })
}

// ---------------------------------------------------------------------------
// Signature verification (XML-DSig: exclusive C14N + RSA PKCS#1 v1.5)
// ---------------------------------------------------------------------------

/// Verify the enveloped signature over the `Assertion` element.
fn verify_assertion_signature(
    xml: &str,
    _doc: &roxmltree::Document,
    assertion: roxmltree::Node,
) -> Result<(), anyhow::Error> {
    // The Signature must be a direct child of the Assertion.
    let signature = assertion
        .children()
        .find(|n| n.tag_name().name() == "Signature" && n.tag_name().namespace() == Some(NS_DS))
        .ok_or_else(|| anyhow::anyhow!("assertion is not signed"))?;

    let signed_info = signature
        .children()
        .find(|n| n.tag_name().name() == "SignedInfo" && n.tag_name().namespace() == Some(NS_DS))
        .ok_or_else(|| anyhow::anyhow!("signature has no SignedInfo"))?;

    // SignatureMethod -> rsa-sha1 | rsa-sha256
    let sig_alg = signed_info
        .children()
        .find(|n| n.tag_name().name() == "SignatureMethod")
        .and_then(|n| n.attribute("Algorithm"))
        .unwrap_or("");
    let sig_hash = HashAlg::from_sig_alg(sig_alg)?;

    // Reference -> URI (#assertion_id), DigestMethod, DigestValue
    let reference = signed_info
        .children()
        .find(|n| n.tag_name().name() == "Reference")
        .ok_or_else(|| anyhow::anyhow!("SignedInfo has no Reference"))?;

    let ref_uri = reference.attribute("URI").unwrap_or("");
    let assertion_id = assertion
        .attribute("ID")
        .ok_or_else(|| anyhow::anyhow!("Assertion has no ID"))?;
    // The reference must point at the assertion we are validating.
    if !ref_uri.is_empty() && ref_uri.trim_start_matches('#') != assertion_id {
        return Err(anyhow::anyhow!(
            "signature Reference URI does not match the Assertion ID"
        ));
    }

    let digest_alg = reference
        .descendants()
        .find(|n| n.tag_name().name() == "DigestMethod")
        .and_then(|n| n.attribute("Algorithm"))
        .unwrap_or("");
    let digest_hash = HashAlg::from_digest_alg(digest_alg)?;

    let digest_value_b64 = reference
        .descendants()
        .find(|n| n.tag_name().name() == "DigestValue")
        .and_then(|n| n.text())
        .ok_or_else(|| anyhow::anyhow!("Reference has no DigestValue"))?
        .trim();

    let signature_value_b64 = signature
        .children()
        .find(|n| n.tag_name().name() == "SignatureValue" && n.tag_name().namespace() == Some(NS_DS))
        .and_then(|n| n.text())
        .ok_or_else(|| anyhow::anyhow!("signature has no SignatureValue"))?
        .trim()
        .split_whitespace()
        .collect::<String>();

    // --- (a) Reference digest: enveloped-signature + exclusive C14N over the
    // Assertion, then compare the digest with DigestValue. ---
    let assertion_xml = &xml[assertion.range()];
    let inherited = ancestor_ns(assertion);
    let assertion_c14n = c14n::canonicalize(assertion_xml, &inherited, /* skip_signature */ true)?;
    let computed_digest = digest_hash.digest(assertion_c14n.as_bytes());
    let expected_digest = config::utils::base64::decode_raw(digest_value_b64)
        .map_err(|e| anyhow::anyhow!("invalid DigestValue base64: {e}"))?;
    if computed_digest != expected_digest {
        return Err(anyhow::anyhow!(
            "assertion digest mismatch (document may have been tampered with)"
        ));
    }

    // --- (b) Signature: exclusive C14N over SignedInfo, then RSA-verify. ---
    let signed_info_xml = &xml[signed_info.range()];
    let si_inherited = ancestor_ns(signed_info);
    let signed_info_c14n = c14n::canonicalize(signed_info_xml, &si_inherited, false)?;
    let sig_bytes = config::utils::base64::decode_raw(&signature_value_b64)
        .map_err(|e| anyhow::anyhow!("invalid SignatureValue base64: {e}"))?;

    let cert_der = load_idp_cert_der()?;
    crypto::rsa_verify(
        &cert_der,
        sig_hash,
        signed_info_c14n.as_bytes(),
        &sig_bytes,
    )?;

    Ok(())
}

/// Collect the namespace declarations in scope at `node` from its ancestors,
/// so exclusive C14N can render any that the subtree visibly utilizes.
fn ancestor_ns(node: roxmltree::Node) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for ns in node.namespaces() {
        let prefix = ns.name().unwrap_or("").to_string();
        if !out.iter().any(|(p, _)| p == &prefix) {
            out.push((prefix, ns.uri().to_string()));
        }
    }
    out
}

/// Load the IdP signing certificate (configured as base64 DER) into DER bytes.
fn load_idp_cert_der() -> Result<Vec<u8>, anyhow::Error> {
    let cfg = get_config();
    let raw = cfg.auth.saml_idp_cert.trim();
    if raw.is_empty() {
        return Err(anyhow::anyhow!("ZO_SAML_IDP_CERT is not set"));
    }
    // Strip PEM header/footer and whitespace if present.
    let b64: String = raw
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<String>()
        .split_whitespace()
        .collect();
    config::utils::base64::decode_raw(&b64)
        .map_err(|e| anyhow::anyhow!("ZO_SAML_IDP_CERT is not valid base64: {e}"))
}

/// Supported digest / signature hash algorithms.
#[derive(Debug, Clone, Copy)]
enum HashAlg {
    Sha1,
    Sha256,
}

impl HashAlg {
    fn from_sig_alg(alg: &str) -> Result<Self, anyhow::Error> {
        if alg.ends_with("rsa-sha256") {
            Ok(HashAlg::Sha256)
        } else if alg.ends_with("rsa-sha1") {
            Ok(HashAlg::Sha1)
        } else {
            Err(anyhow::anyhow!("unsupported SignatureMethod: {alg}"))
        }
    }

    fn from_digest_alg(alg: &str) -> Result<Self, anyhow::Error> {
        if alg.ends_with("#sha256") {
            Ok(HashAlg::Sha256)
        } else if alg.ends_with("#sha1") {
            Ok(HashAlg::Sha1)
        } else {
            Err(anyhow::anyhow!("unsupported DigestMethod: {alg}"))
        }
    }

    fn digest(&self, data: &[u8]) -> Vec<u8> {
        use sha1::{Digest, Sha1};
        use sha2::Sha256;
        match self {
            HashAlg::Sha1 => Sha1::digest(data).to_vec(),
            HashAlg::Sha256 => Sha256::digest(data).to_vec(),
        }
    }
}

mod crypto {
    use rsa::{Pkcs1v15Sign, RsaPublicKey, pkcs1::DecodeRsaPublicKey};
    use x509_parser::prelude::*;

    use super::HashAlg;

    /// RSA PKCS#1 v1.5 verify `signature` over `message` using the public key
    /// embedded in the X.509 `cert_der`.
    pub fn rsa_verify(
        cert_der: &[u8],
        hash: HashAlg,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), anyhow::Error> {
        let (_, cert) = X509Certificate::from_der(cert_der)
            .map_err(|e| anyhow::anyhow!("failed to parse IdP certificate: {e}"))?;
        // For RSA keys the SubjectPublicKey bitstring is a PKCS#1 RSAPublicKey DER.
        let spki = cert.public_key();
        let pkcs1 = spki.subject_public_key.data.as_ref();
        let public_key = RsaPublicKey::from_pkcs1_der(pkcs1)
            .map_err(|e| anyhow::anyhow!("IdP key is not an RSA public key: {e}"))?;

        let (scheme, hashed) = match hash {
            HashAlg::Sha1 => {
                use sha1::{Digest, Sha1};
                (Pkcs1v15Sign::new::<Sha1>(), Sha1::digest(message).to_vec())
            }
            HashAlg::Sha256 => {
                use sha2::{Digest, Sha256};
                (
                    Pkcs1v15Sign::new::<Sha256>(),
                    Sha256::digest(message).to_vec(),
                )
            }
        };

        public_key
            .verify(scheme, &hashed, signature)
            .map_err(|e| anyhow::anyhow!("SAML signature verification failed: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Exclusive XML Canonicalization (http://www.w3.org/2001/10/xml-exc-c14n#)
// ---------------------------------------------------------------------------

mod c14n {
    use quick_xml::{
        events::{BytesStart, Event},
        reader::Reader,
    };

    /// Canonicalize `xml` (a single element subtree) using exclusive C14N.
    ///
    /// `inherited` provides prefix->uri bindings visible from ancestors (used
    /// to render namespaces the subtree visibly utilizes but does not declare).
    /// When `skip_signature` is set, the first `ds:Signature` element (the
    /// enveloped signature) and its subtree are omitted — this implements the
    /// enveloped-signature transform for the Reference digest.
    pub fn canonicalize(
        xml: &str,
        inherited: &[(String, String)],
        skip_signature: bool,
    ) -> Result<String, anyhow::Error> {
        // Default config preserves whitespace text nodes, which C14N requires.
        let mut reader = Reader::from_str(xml);

        let mut out = String::new();
        // Two stacks, per the exclusive-C14N algorithm:
        //  * scope_stack  — namespace *declarations* in scope (for resolving a
        //    prefix to its uri). Seeded with the inherited ancestor context so
        //    prefixes declared outside this subtree still resolve.
        //  * rendered_stack — namespaces actually *emitted* into the output
        //    ancestor chain (for the "already rendered" suppression check).
        //    Starts empty: the subtree is canonicalized standalone, so a
        //    visibly-utilized namespace must be rendered at first use even if
        //    it was in scope in the original document.
        let mut scope_stack: Vec<Vec<(String, String)>> = vec![inherited.to_vec()];
        let mut rendered_stack: Vec<Vec<(String, String)>> = Vec::new();
        // Depth at which we entered a skipped ds:Signature; None if not skipping.
        let mut skip_depth: Option<usize> = None;
        let mut depth: usize = 0;

        loop {
            match reader
                .read_event()
                .map_err(|e| anyhow::anyhow!("c14n: XML read error: {e}"))?
            {
                Event::Start(e) => {
                    depth += 1;
                    if skip_depth.is_some() {
                        continue;
                    }
                    if skip_signature && is_ds_signature(&e) {
                        skip_depth = Some(depth);
                        continue;
                    }
                    let (scope_frame, rendered_frame) =
                        render_start(&e, &scope_stack, &rendered_stack, &mut out)?;
                    scope_stack.push(scope_frame);
                    rendered_stack.push(rendered_frame);
                }
                Event::End(e) => {
                    if let Some(d) = skip_depth {
                        if depth == d {
                            skip_depth = None;
                        }
                        depth -= 1;
                        continue;
                    }
                    let qname = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    out.push_str("</");
                    out.push_str(&qname);
                    out.push('>');
                    scope_stack.pop();
                    rendered_stack.pop();
                    depth -= 1;
                }
                Event::Empty(e) => {
                    depth += 1;
                    if skip_depth.is_some() {
                        depth -= 1;
                        continue;
                    }
                    if skip_signature && is_ds_signature(&e) {
                        depth -= 1;
                        continue;
                    }
                    // Self-closing: render, emit end tag; no children to scope.
                    let _ = render_start(&e, &scope_stack, &rendered_stack, &mut out)?;
                    let qname = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    out.push_str("</");
                    out.push_str(&qname);
                    out.push('>');
                    depth -= 1;
                }
                Event::Text(e) => {
                    if skip_depth.is_some() {
                        continue;
                    }
                    // Decode bytes->str, unescape entity references, then
                    // re-escape per C14N rules.
                    let decoded = e
                        .decode()
                        .map_err(|err| anyhow::anyhow!("c14n: text decode error: {err}"))?;
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .map(|c| c.into_owned())
                        .unwrap_or_else(|_| decoded.to_string());
                    out.push_str(&escape_text(&unescaped));
                }
                Event::CData(e) => {
                    if skip_depth.is_some() {
                        continue;
                    }
                    // C14N converts CDATA into escaped character data.
                    let s = String::from_utf8_lossy(e.as_ref()).to_string();
                    out.push_str(&escape_text(&s));
                }
                Event::Eof => break,
                // Comments are removed by C14N; declarations/PIs ignored here.
                _ => {}
            }
        }
        Ok(out)
    }

    fn is_ds_signature(e: &BytesStart) -> bool {
        let name = e.name();
        let local = name.local_name();
        local.as_ref() == b"Signature"
    }

    /// Render an element start tag per exclusive C14N. Returns
    /// `(scope_frame, rendered_frame)`: the element's local namespace
    /// declarations (pushed onto the scope stack for descendant resolution)
    /// and the namespaces actually emitted (pushed onto the rendered stack).
    fn render_start(
        e: &BytesStart,
        scope_stack: &[Vec<(String, String)>],
        rendered_stack: &[Vec<(String, String)>],
        out: &mut String,
    ) -> Result<(Vec<(String, String)>, Vec<(String, String)>), anyhow::Error> {
        let qname = String::from_utf8_lossy(e.name().as_ref()).to_string();
        let elem_prefix = prefix_of(&qname);

        // Collect this element's own xmlns declarations and plain attributes.
        let mut local_ns: Vec<(String, String)> = Vec::new(); // (prefix, uri)
        let mut plain_attrs: Vec<(String, String, String)> = Vec::new(); // (prefix, local, value)
        let mut utilized: Vec<String> = Vec::new(); // visibly-utilized prefixes

        if !elem_prefix.is_empty() {
            utilized.push(elem_prefix.clone());
        } else {
            // default namespace is visibly utilized by an unprefixed element
            utilized.push(String::new());
        }

        for attr in e.attributes().with_checks(false) {
            let attr = attr.map_err(|err| anyhow::anyhow!("c14n: bad attribute: {err}"))?;
            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
            let value_raw = String::from_utf8_lossy(&attr.value).to_string();
            // Unescape entity references so C14N can re-escape canonically.
            let value = quick_xml::escape::unescape(&value_raw)
                .map(|c| c.to_string())
                .unwrap_or(value_raw);

            if key == "xmlns" {
                local_ns.push((String::new(), value));
            } else if let Some(p) = key.strip_prefix("xmlns:") {
                local_ns.push((p.to_string(), value));
            } else {
                let p = prefix_of(&key);
                let local = local_of(&key);
                // Prefixed attributes visibly utilize their prefix. Unprefixed
                // attributes are NOT in any namespace (default ns does not
                // apply to attributes), so they do not utilize anything.
                if !p.is_empty() {
                    utilized.push(p.clone());
                }
                plain_attrs.push((p, local, value));
            }
        }

        // Build the scope frame (local declarations) so descendants resolve
        // prefixes declared here, and determine which namespaces to emit.
        let scope_frame = local_ns.clone();

        // For each visibly-utilized prefix, emit its binding unless the same
        // prefix->uri is already present in the rendered-output ancestor chain.
        let mut render_ns: Vec<(String, String)> = Vec::new();
        for p in &utilized {
            // Resolve uri: local decl wins, else scope (ancestors + inherited).
            let uri = local_ns
                .iter()
                .find(|(pp, _)| pp == p)
                .map(|(_, u)| u.clone())
                .or_else(|| lookup(scope_stack, p));
            let uri = match uri {
                Some(u) if !u.is_empty() => u,
                _ => continue, // no binding (e.g. unprefixed with no default ns)
            };
            if !rendered_has(rendered_stack, p, &uri)
                && !render_ns.iter().any(|(pp, _)| pp == p)
            {
                render_ns.push((p.clone(), uri));
            }
        }

        // Sort namespaces by prefix (default ns, empty prefix, sorts first).
        render_ns.sort_by(|a, b| a.0.cmp(&b.0));
        // Sort plain attributes by (namespace uri, local name).
        plain_attrs.sort_by(|a, b| {
            let ua = if a.0.is_empty() {
                String::new()
            } else {
                lookup(scope_stack, &a.0).unwrap_or_default()
            };
            let ub = if b.0.is_empty() {
                String::new()
            } else {
                lookup(scope_stack, &b.0).unwrap_or_default()
            };
            ua.cmp(&ub).then(a.1.cmp(&b.1))
        });

        out.push('<');
        out.push_str(&qname);
        for (p, uri) in &render_ns {
            if p.is_empty() {
                out.push_str(" xmlns=\"");
            } else {
                out.push_str(" xmlns:");
                out.push_str(p);
                out.push_str("=\"");
            }
            out.push_str(&escape_attr(uri));
            out.push('"');
        }
        for (p, local, value) in &plain_attrs {
            out.push(' ');
            if !p.is_empty() {
                out.push_str(p);
                out.push(':');
            }
            out.push_str(local);
            out.push_str("=\"");
            out.push_str(&escape_attr(value));
            out.push('"');
        }
        out.push('>');

        Ok((scope_frame, render_ns))
    }

    fn prefix_of(qname: &str) -> String {
        match qname.split_once(':') {
            Some((p, _)) => p.to_string(),
            None => String::new(),
        }
    }

    fn local_of(qname: &str) -> String {
        match qname.split_once(':') {
            Some((_, l)) => l.to_string(),
            None => qname.to_string(),
        }
    }

    /// Resolve the uri currently bound to `prefix` (nearest frame wins).
    fn lookup(stack: &[Vec<(String, String)>], prefix: &str) -> Option<String> {
        for frame in stack.iter().rev() {
            if let Some((_, uri)) = frame.iter().find(|(p, _)| p == prefix) {
                return Some(uri.clone());
            }
        }
        None
    }

    /// Whether `prefix`->`uri` is already present in the rendered-output
    /// ancestor chain (nearest binding for the prefix equals `uri`).
    fn rendered_has(stack: &[Vec<(String, String)>], prefix: &str, uri: &str) -> bool {
        match lookup(stack, prefix) {
            Some(u) => u == uri,
            None => false,
        }
    }

    fn escape_text(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '\r' => out.push_str("&#xD;"),
                _ => out.push(c),
            }
        }
        out
    }

    fn escape_attr(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '"' => out.push_str("&quot;"),
                '\t' => out.push_str("&#x9;"),
                '\n' => out.push_str("&#xA;"),
                '\r' => out.push_str("&#xD;"),
                _ => out.push(c),
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

fn utc_now_iso8601() -> String {
    use chrono::{SecondsFormat, Utc};
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Standard base64 of raw bytes (the `config::utils::base64::encode` helper
/// only accepts `&str`, so encode bytes via the base64 crate directly).
fn b64_encode_bytes(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn xml_escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn xml_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
