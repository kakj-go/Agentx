ALTER TABLE skills
    ADD COLUMN alias VARCHAR(160) NULL AFTER name;

UPDATE skills
SET alias = name
WHERE alias IS NULL;

ALTER TABLE skills
    MODIFY COLUMN alias VARCHAR(160) NOT NULL,
    ADD UNIQUE KEY uq_skill_alias (tenant_id, alias);
