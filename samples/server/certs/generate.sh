#!/bin/sh
# Regenerates every certificate the sample server uses. Needs OpenSSL 3.4 or newer (for
# -not_before and -not_after). The output is committed, so running the server never needs
# OpenSSL; run this only to refresh the files, for example before they expire.
#
# These keys are public test fixtures. They protect nothing and must never be trusted anywhere.
set -eu
cd "$(dirname "$0")"

# SAN of a certificate that is correct for the local server.
LOCAL_SAN="subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1"
VALID_FROM="20260101000000Z"
VALID_TO="21260101000000Z"

key() {
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1.key"
}

# A self-signed leaf (not a CA, so it cannot vouch for itself as an issuer).
self_signed() { # name, common name, san, not_before, not_after
    key "$1"
    openssl req -x509 -new -key "$1.key" -subj "/CN=$2" \
        -not_before "$4" -not_after "$5" \
        -addext "$3" -addext "basicConstraints=critical,CA:FALSE" \
        -addext "keyUsage=critical,digitalSignature" \
        -addext "extendedKeyUsage=serverAuth" -out "$1.pem"
}

self_signed self-signed localhost "$LOCAL_SAN" "$VALID_FROM" "$VALID_TO"
self_signed expired localhost "$LOCAL_SAN" "20200101000000Z" "20200102000000Z"
self_signed not-yet-valid localhost "$LOCAL_SAN" "21000101000000Z" "21260101000000Z"
self_signed wrong-host other.example.test "subjectAltName=DNS:other.example.test" \
    "$VALID_FROM" "$VALID_TO"

# A private CA and a leaf signed by it. Postino does not trust the CA, so the leaf is still
# rejected as an unknown issuer; ca.pem is there for when custom CAs are supported.
key ca
openssl req -x509 -new -key ca.key -subj "/CN=Postino Sample CA" \
    -not_before "$VALID_FROM" -not_after "$VALID_TO" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -out ca.pem
key private-ca
openssl req -new -key private-ca.key -subj "/CN=localhost" -out private-ca.csr
openssl x509 -req -in private-ca.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
    -not_before "$VALID_FROM" -not_after "$VALID_TO" \
    -extfile /dev/stdin -out private-ca.pem <<EXT
subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth
EXT
rm -f private-ca.csr ca.srl
# The CA key is only needed to sign; keep it out of the repository.
rm -f ca.key
