ALTER TABLE users
    MODIFY COLUMN status ENUM('invited', 'active', 'disabled') NOT NULL DEFAULT 'invited';

UPDATE users
SET status = 'invited'
WHERE status = 'active' AND password_change_required = TRUE;

CREATE TABLE auth_login_attempts (
    login_key CHAR(64) NOT NULL,
    failure_count INT UNSIGNED NOT NULL DEFAULT 0,
    window_started_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    locked_until TIMESTAMP(6) NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (login_key),
    KEY idx_auth_login_attempts_locked (locked_until)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE outbox_events
    ADD COLUMN locked_by BINARY(16) NULL AFTER last_error,
    ADD COLUMN locked_until TIMESTAMP(6) NULL AFTER locked_by,
    ADD KEY idx_outbox_lease (published_at, available_at, locked_until);
