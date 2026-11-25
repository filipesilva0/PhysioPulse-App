-- PhysioPulse plan, phase 5: the staff iPhone app.
--
-- A device is a computer or a phone. The phone app's Rust core says so (`X-Device-Kind: phone`) when it first asks
-- to be approved, and the owner sees "iPhone" in the list before approving it. The kind is fixed from then on.
--
-- Sessions on an approved phone (owner, 11 Nov 2026): no idle timeout (the app asks for Face ID every time it
-- reopens instead), and they end after 30 days, when the phone asks for the password and the two-step code again.
-- The token is kept in the iPhone's keychain behind Face ID. The owner can sign a phone out from Team.
ALTER TABLE devices ADD COLUMN kind TEXT NOT NULL DEFAULT 'computer' CHECK (kind IN ('computer', 'phone'));
ALTER TABLE sessions ADD COLUMN phone BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX sessions_phone ON sessions (staff_id) WHERE phone;
