import test from "node:test";
import assert from "node:assert/strict";

import type { TitleClaimRecord } from "../types/request-rule-sets.ts";
import { liveOperatorKeep, requestRuleReadsRequester } from "./request-rule-sets.ts";

test("a matcher reading requester fields raises the permission hint", () => {
  assert.equal(
    requestRuleReadsRequester(
      'deny if {\n  input.requester.username == "sample-member"\n}',
    ),
    true,
  );
  assert.equal(
    requestRuleReadsRequester(
      'approve if {\n  "auto_approve" in input["requester"].library_permissions\n}',
    ),
    true,
  );
});

test("requester mentioned only in a comment or another field does not", () => {
  assert.equal(
    requestRuleReadsRequester(
      '# input.requester is not read here\napprove if {\n  input.request.origin == "web"\n}',
    ),
    false,
  );
  assert.equal(
    requestRuleReadsRequester('manual if {\n  input.request.origin == "api"\n}'),
    false,
  );
});

function claim(overrides: Partial<TitleClaimRecord>): TitleClaimRecord {
  return {
    id: "claim-a",
    titleId: "title-a",
    libraryId: "library-a",
    producer: "OPERATOR_KEEP",
    producerRef: null,
    kind: "KEEP",
    state: "ACTIVE",
    durationDays: null,
    startsAt: null,
    expiresAt: null,
    createdBy: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    releasedReason: null,
    ...overrides,
  };
}

test("only a live operator keep counts as kept", () => {
  assert.equal(liveOperatorKeep(undefined), null);
  assert.equal(
    liveOperatorKeep([
      claim({ id: "released", state: "RELEASED" }),
      claim({ id: "request-keep", producer: "REQUEST_PERMANENT" }),
    ]),
    null,
  );
  assert.equal(
    liveOperatorKeep([
      claim({ id: "released", state: "RELEASED" }),
      claim({ id: "live" }),
    ])?.id,
    "live",
  );
});
