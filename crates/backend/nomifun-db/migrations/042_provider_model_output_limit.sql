-- Declare each model's output-token ceiling next to its context window.
-- NULL is meaningful: serializers that support omission let the provider choose.
ALTER TABLE provider_models
    ADD COLUMN output_limit INTEGER
    CHECK (output_limit IS NULL OR output_limit > 0);

-- Output ceilings are typed model data now. Remove every legacy request-body
-- spelling from params before the resolver starts rejecting these no-ops.
UPDATE provider_models
   SET params = json_remove(
       params,
       '$.max_tokens',
       '$.max_completion_tokens',
       '$.maxOutputTokens',
       '$.max_output_tokens',
       '$.generationConfig.maxOutputTokens',
       '$._flowy_catalog_max_tokens'
   );

-- A compatible OpenAI endpoint may have named a different top-level field via
-- max_tokens_field. Keep max_tokens_field itself (it still selects the typed
-- serializer field); only its old untyped value goes.
UPDATE provider_models
   SET params = json_remove(
       params,
       '$.' || json_quote(json_extract(params, '$.max_tokens_field'))
   )
 WHERE json_type(params, '$.max_tokens_field') = 'text'
   AND trim(json_extract(params, '$.max_tokens_field')) <> '';

-- Anthropic / Bedrock / Vertex require max_tokens on the wire. Preserve the
-- effective 8192 value the desktop agent sent before this migration.
-- Local protocol names are short forms (not anthropic.messages).
UPDATE provider_models
   SET output_limit = 8192
 WHERE protocol IN ('anthropic', 'bedrock', 'vertex');
