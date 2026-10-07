schema = {
  "$schema" = "https://json-schema.org/draft/2020-12/schema"
  "$id"     = "riverbase:form:DocumentSpec"
  type      = "object"
  required  = ["key", "title"]
  properties = {
    key = {
      type    = "string"
      pattern = "^[A-Za-z]{3}-\\d{4}$"
    }
    title   = { type = "string" }
    desc    = { type = "string" }
    version = { type = "integer" }
    types   = { type = "string" }
    nodes = {
      type  = "array"
      items = { "$ref" = "#/$defs/documentNode" }
    }
  }
  additionalProperties = false
  "$defs" = {
    documentNode = {
      oneOf = [
        { "$ref" = "#/$defs/sectionNode" },
        { "$ref" = "#/$defs/contentNode" },
        { "$ref" = "#/$defs/formNode" },
      ]
    }
    sectionNode = {
      type     = "object"
      required = ["node_type", "title"]
      properties = {
        node_type = { const = "section" }
        title     = { type = "string" }
        desc      = { type = "string" }
        order     = { type = "integer" }
        children  = {
          type  = "array"
          items = { "$ref" = "#/$defs/documentNode" }
        }
      }
      additionalProperties = false
    }
    contentNode = {
      type     = "object"
      required = ["node_type", "title"]
      properties = {
        node_type = { const = "content" }
        title     = { type = "string" }
        content   = { type = "string" }
        ctype     = { type = "string" }
        order     = { type = "integer" }
      }
      additionalProperties = false
    }
    formNode = {
      type     = "object"
      required = ["node_type", "form_key"]
      properties = {
        node_type = { const = "form" }
        form_key  = { type = "string" }
        title     = { type = "string" }
        order     = { type = "integer" }
        attrs     = {
          type                 = "object"
          additionalProperties = true
        }
        data = {
          type                 = "object"
          additionalProperties = true
        }
      }
      additionalProperties = false
    }
  }
}
