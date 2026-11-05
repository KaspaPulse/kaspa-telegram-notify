-- OP010: expand ONLY the pending_rewards status constraint for strict Coinbase proof.
-- No row updates or deletes. Safe to retain if a release must be rolled back.
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '20s';

DO $guard$
DECLARE found text;
BEGIN
    SELECT pg_get_constraintdef(oid) INTO found
      FROM pg_constraint
     WHERE conrelid='public.pending_rewards'::regclass
       AND conname='ck_pending_rewards_status';
    IF found IS DISTINCT FROM
      'CHECK ((status = ANY (ARRAY[''pending''::text, ''confirmed''::text, ''failed''::text, ''expired''::text])))'
    THEN
      RAISE EXCEPTION 'HOLD unexpected pending_rewards status constraint: %', found;
    END IF;
END $guard$;

ALTER TABLE public.pending_rewards DROP CONSTRAINT ck_pending_rewards_status;
ALTER TABLE public.pending_rewards
  ADD CONSTRAINT ck_pending_rewards_status
  CHECK (status IN ('pending','confirmed','failed','expired','awaiting_block_proof'));
COMMIT;
