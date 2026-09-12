import { getRuntimeBackendUrl } from "@/lib/runtime-config";

type AuthlessProof = {
  proof: string;
  expiresAt: number;
};

let cachedProof: AuthlessProof | null = null;
// Every /authless-client response sets a fresh cookie, and the server only
// accepts a proof that matches the cookie the browser holds. Concurrent
// startup queries must share one fetch, or the cookie jar can keep one
// response while the cache keeps the other and every request is rejected.
let inFlightProof: Promise<string | null> | null = null;

function authlessClientUrl() {
  return getRuntimeBackendUrl("/authless-client");
}

export async function getAuthlessWebClientProof(): Promise<string | null> {
  const nowSeconds = Math.floor(Date.now() / 1000);
  if (cachedProof && cachedProof.expiresAt - 15 > nowSeconds) {
    return cachedProof.proof;
  }
  if (!inFlightProof) {
    inFlightProof = fetchAuthlessWebClientProof().finally(() => {
      inFlightProof = null;
    });
  }
  return inFlightProof;
}

async function fetchAuthlessWebClientProof(): Promise<string | null> {
  try {
    const response = await fetch(authlessClientUrl(), {
      method: "GET",
      cache: "no-store",
      credentials: "include",
      headers: { accept: "application/json" },
    });
    if (!response.ok) {
      cachedProof = null;
      return null;
    }
    const body = (await response.json()) as Partial<AuthlessProof>;
    if (typeof body.proof !== "string" || typeof body.expiresAt !== "number") {
      cachedProof = null;
      return null;
    }
    cachedProof = { proof: body.proof, expiresAt: body.expiresAt };
    return cachedProof.proof;
  } catch {
    cachedProof = null;
    return null;
  }
}
