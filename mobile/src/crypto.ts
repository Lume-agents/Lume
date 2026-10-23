// The Lume protocol's encryption, unchanged from the first mobile app: AES-256-GCM with a key derived
// from the pairing code (while pairing) or the device token (afterwards), and a purpose-bound AAD.

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

export type Envelope = { nonce: string; ciphertext: string };

export const randomBytes = (length: number) => crypto.getRandomValues(new Uint8Array(length));

export const encodeBytes = (value: ArrayBuffer | Uint8Array) =>
  btoa(String.fromCharCode(...new Uint8Array(value)))
    .replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");

export const decodeBytes = (value: string) => {
  const base64 = value.replaceAll("-", "+").replaceAll("_", "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  return Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
};

export async function keyFromSecret(secret: string) {
  const digest = await crypto.subtle.digest("SHA-256", textEncoder.encode(secret));
  return crypto.subtle.importKey("raw", digest, "AES-GCM", false, ["encrypt", "decrypt"]);
}

export async function encryptPayload(key: CryptoKey, payload: unknown, aad: string): Promise<Envelope> {
  const nonce = randomBytes(12);
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv: nonce, additionalData: textEncoder.encode(aad) },
    key,
    textEncoder.encode(JSON.stringify(payload)),
  );
  return { nonce: encodeBytes(nonce), ciphertext: encodeBytes(ciphertext) };
}

export async function decryptPayload<T = unknown>(key: CryptoKey, envelope: Envelope, aad: string): Promise<T> {
  const plaintext = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: decodeBytes(envelope.nonce), additionalData: textEncoder.encode(aad) },
    key,
    decodeBytes(envelope.ciphertext),
  );
  return JSON.parse(textDecoder.decode(plaintext)) as T;
}
