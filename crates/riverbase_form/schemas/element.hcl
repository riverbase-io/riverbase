schema = {
  "$schema" = "https://json-schema.org/draft/2020-12/schema"
  "$id"     = "riverbase:form:ElementSpec"
  type      = "object"
  required  = ["key", "title", "table_name", "field"]
  properties = {
    key = {
      type    = "string"
      pattern = "^[A-Za-z]{3}-\\d{4}$"
    }
    title      = { type = "string" }
    desc       = { type = "string" }
    table_name = { type = "string" }
    validation = { "$ref" = "#/$defs/validationBlock" }
    constraint = {
      type                 = "object"
      additionalProperties = { "$ref" = "#/$defs/constraintRule" }
    }
    field = {
      type = "object"
      additionalProperties = { "$ref" = "#/$defs/fieldDef" }
    }
  }
  additionalProperties = false
  "$defs" = {
    validationBlock = {
      type = "object"
      properties = {
        message = { type = "string" }
      }
      additionalProperties = true
    }
    constraintRule = {
      type     = "object"
      required = ["expr"]
      properties = {
        expr    = { type = "string" }
        message = { type = "string" }
      }
      additionalProperties = false
    }
    fieldDef = {
      type     = "object"
      required = ["type"]
      properties = {
        type       = { type = "string" }
        format     = { type = "string" }
        required   = { type = "boolean" }
        validation = { "$ref" = "#/$defs/validationBlock" }
        constraint = {
          type                 = "object"
          additionalProperties = { "$ref" = "#/$defs/constraintRule" }
        }
      }
      additionalProperties = false
    }
  }
}
