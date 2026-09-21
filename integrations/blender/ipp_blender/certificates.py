"""Local credentials, generated once without an external executable."""

from datetime import datetime, timedelta, timezone
import ipaddress
import os
from pathlib import Path
import ssl

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID


def credentials(directory, certificate="", private_key=""):
    if bool(certificate) != bool(private_key):
        raise ValueError("Configure both the certificate and private key")
    if certificate:
        pair = Path(certificate).expanduser(), Path(private_key).expanduser()
    else:
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        pair = directory / "localhost.pem", directory / "localhost-key.pem"
        if not any(path.exists() for path in pair):
            generate(*pair)
        elif not all(path.is_file() for path in pair):
            raise ValueError(
                "Local TLS credentials are incomplete; configure a valid pair"
            )
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(*map(str, pair))
    return context, pair


def generate(certificate, private_key):
    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "IPP localhost")])
    now = datetime.now(timezone.utc)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(minutes=5))
        .not_valid_after(now + timedelta(days=365))
        .add_extension(
            x509.SubjectAlternativeName(
                [
                    x509.DNSName("localhost"),
                    x509.IPAddress(ipaddress.ip_address("127.0.0.1")),
                    x509.IPAddress(ipaddress.ip_address("::1")),
                ]
            ),
            critical=False,
        )
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(
            x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False
        )
        .sign(key, hashes.SHA256())
    )
    # Exclusive files prevent an accidental overwrite of a supplied identity.
    with os.fdopen(
        os.open(private_key, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as stream:
        stream.write(
            key.private_bytes(
                serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        )
    with open(certificate, "xb") as stream:
        stream.write(cert.public_bytes(serialization.Encoding.PEM))
