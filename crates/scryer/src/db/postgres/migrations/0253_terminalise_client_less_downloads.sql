-- Finish canonical downloads whose client config is gone.
--
-- Deleting a download client config ended its bindings but left the `downloads`
-- rows live, and `terminal_at` had no writer at all. A client that is re-added
-- gets a new config id while the physical client still lists the same native
-- items carrying the tokens minted under the old config, so the observation
-- resolver kept finding a token whose binding was ended under a config that no
-- longer exists. Before the resolver learned to tell an ended *binding* from a
-- finished *download*, that pair resolved to
-- `Conflict { token_id: T, binding_download_id: T }` — a download conflicting
-- with itself — on every poll of every such row, forever. On a load-test
-- instance that was 961 rows, ~1,100 registry write transactions a second and
-- ~1,650 WARN lines a second.
--
-- The resolver now rebinds an ended binding whose download is still live, which
-- is the right answer for a client the user re-added on purpose. It is the
-- wrong answer for these rows: their client config was deleted, which is Scryer
-- deciding it was done with them, and their submissions went with it. So they
-- are marked terminal here, the way `delete_download_client_config` now does at
-- the time of the delete.
--
-- Idempotent: `terminal_at IS NULL` means a re-run touches nothing, and rows
-- that gained an active binding in the meantime are excluded.
UPDATE downloads
SET terminal_at = CURRENT_TIMESTAMP
WHERE terminal_at IS NULL
  -- Its last binding is ended and named a client config that is gone.
  AND EXISTS (
        SELECT 1
        FROM download_client_bindings b
        WHERE b.download_id = downloads.id
          AND b.ended_at IS NOT NULL
          AND TRIM(COALESCE(b.client_config_id, '')) <> ''
          AND NOT EXISTS (
                SELECT 1
                FROM download_clients c
                WHERE c.id = b.client_config_id
              )
      )
  -- Nothing holds it actively: a download some client is still reporting is
  -- live, whatever else is true of it.
  AND NOT EXISTS (
        SELECT 1
        FROM download_client_bindings active
        WHERE active.download_id = downloads.id
          AND active.ended_at IS NULL
      );
