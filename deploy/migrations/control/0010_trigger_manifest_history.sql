-- Identical trigger manifests recur across revisions (e.g. deleting the last
-- channel returns to the empty manifest), so manifest content must not be
-- unique per application.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE application_runtime_trigger_revisions
    DROP INDEX uq_control_trigger_manifest_hash;
