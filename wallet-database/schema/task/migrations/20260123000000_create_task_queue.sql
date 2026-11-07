-- Create task_queue table with complete structure for task.db
CREATE TABLE IF NOT EXISTS task_queue (
    id VARCHAR(64) NOT NULL,
    task_name VARCHAR(64) NOT NULL,
    request_body TEXT DEFAULT '' NOT NULL,
    type INTEGER NOT NULL,
    retry_times INTEGER NOT NULL DEFAULT 0,
    status INTEGER NOT NULL DEFAULT 0,
    err_msg TEXT DEFAULT NULL,
    remark TEXT DEFAULT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP,
    PRIMARY KEY (id)
);

-- Add indexes for better performance
CREATE INDEX IF NOT EXISTS idx_task_queue_name_status ON task_queue(task_name, status);
CREATE INDEX IF NOT EXISTS idx_task_queue_type_status ON task_queue(type, status);
CREATE INDEX IF NOT EXISTS idx_task_queue_created_at ON task_queue(created_at);
CREATE INDEX IF NOT EXISTS idx_task_queue_status ON task_queue(status);
