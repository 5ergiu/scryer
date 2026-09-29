-- Manage Lists joined the full-admin permission set (bit 4, value 16). Stored
-- masks predate it, so every existing administrator lost full-admin status and
-- startup refused to boot with form login enabled. Grant it to every account
-- that already holds the four earlier full-admin permissions (mask 15); a
-- narrower mask is not a full admin and stays as it is.
UPDATE user_app_permission_masks
SET permission_mask = permission_mask | 16,
    updated_at = now()
WHERE (permission_mask & 15) = 15
  AND (permission_mask & 16) = 0;
