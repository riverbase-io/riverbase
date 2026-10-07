schema = {
  "$schema" = "https://json-schema.org/draft/2020-12/schema"
  "$id"     = "riverbase:form:FormSpec"
  type      = "object"
  required  = ["key", "title"]
  properties = {
    key = {
      type    = "string"
      pattern = "^[A-Za-z]{3}-\\d{4}$"
    }
    title  = { type = "string" }
    desc   = { type = "string" }
    header = { type = "string" }
    footer = { type = "string" }
    element = {
      type                 = "object"
      additionalProperties = { "$ref" = "#/$defs/formElement" }
    }
    group = {
      type                 = "object"
      additionalProperties = { "$ref" = "#/$defs/formGroup" }
    }
    constraint = {
      type                 = "object"
      additionalProperties = { "$ref" = "#/$defs/formConstraintRule" }
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
    formConstraintRule = {
      type = "object"
      properties = {
        expr    = { type = "string" }
        message = { type = "string" }
      }
      additionalProperties = true
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
    inlineElement = {
      type = "object"
      properties = {
        validation = { "$ref" = "#/$defs/validationBlock" }
        constraint = {
          type                 = "object"
          additionalProperties = { "$ref" = "#/$defs/constraintRule" }
        }
        field = {
          type                 = "object"
          additionalProperties = { "$ref" = "#/$defs/fieldDef" }
        }
      }
      additionalProperties = false
    }
    formElement = {
      type = "object"
      properties = {
        key = {
          type    = "string"
          pattern = "^[A-Za-z]{3}-\\d{4}$"
        }
        required = { type = "boolean" }
        title    = { type = "string" }
        desc     = { type = "string" }
        schema   = { "$ref" = "#/$defs/inlineElement" }
      }
      additionalProperties = false
    }
    formGroup = {
      type = "object"
      properties = {
        title = { type = "string" }
        desc  = { type = "string" }
        element = {
          type                 = "object"
          additionalProperties = { "$ref" = "#/$defs/formElement" }
        }
      }
      additionalProperties = false
    }
  }
}
