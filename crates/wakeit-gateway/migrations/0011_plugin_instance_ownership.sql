-- Runner-created instances have no delegated workspace principal until an administrator grants one.
ALTER TABLE plugin_instances ALTER COLUMN managed_by DROP NOT NULL;
