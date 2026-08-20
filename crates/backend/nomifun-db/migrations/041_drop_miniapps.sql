-- Mini-apps product surface was removed from the UI; drop the published-snapshot
-- table. Working copies under {work_dir}/miniapps/ are not managed dataset roots
-- and are left on disk.
DROP TABLE IF EXISTS miniapps;
