-- A title holds one live operator keep. Keep clicks that raced each other,
-- and leases converted to permanent on a title that already had a keep, left
-- several; releasing one of them still left the title held by another. The
-- oldest live keep stays and the rest are released, so every title keeps
-- exactly the protection it had.
UPDATE lifecycle_claims
SET state = 'released',
    released_reason = 'duplicate_operator_keep',
    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE producer = 'operator_keep'
  AND state IN ('dormant', 'active')
  AND EXISTS (
      SELECT 1
      FROM lifecycle_claims older
      WHERE older.title_id = lifecycle_claims.title_id
        AND older.producer = 'operator_keep'
        AND older.state IN ('dormant', 'active')
        AND (
            older.created_at < lifecycle_claims.created_at
            OR (older.created_at = lifecycle_claims.created_at AND older.id < lifecycle_claims.id)
        )
  );

-- The live states are listed literally, as in idx_lifecycle_claims_live_producer.
CREATE UNIQUE INDEX idx_lifecycle_claims_live_operator_keep
    ON lifecycle_claims(title_id)
    WHERE producer = 'operator_keep' AND state IN ('dormant', 'active');
