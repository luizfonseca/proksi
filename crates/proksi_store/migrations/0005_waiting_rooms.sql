-- Waiting rooms (the `waiting_room` plugin): the visitors of each room holding a slot, and
-- the others in the order they came. Times are milliseconds since the epoch, by the database's
-- clock.

CREATE TABLE IF NOT EXISTS proksi_waiting_rooms (
    room TEXT PRIMARY KEY,
    -- The last ticket given; changing it locks the room
    next_ticket BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS proksi_waiting_room_visitors (
    room TEXT NOT NULL,
    visitor TEXT NOT NULL,
    ticket BIGINT NOT NULL,
    -- When their slot expires, or NULL while they wait
    expires_at BIGINT,
    -- When they last called (waiting visitors who stop calling lose their place)
    seen_at BIGINT NOT NULL,
    PRIMARY KEY (room, visitor)
);

CREATE INDEX IF NOT EXISTS proksi_waiting_room_queue
    ON proksi_waiting_room_visitors (room, ticket);

-- Keys proksi generates once and shares between replicas, as hex
CREATE TABLE IF NOT EXISTS proksi_keys (
    name TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
