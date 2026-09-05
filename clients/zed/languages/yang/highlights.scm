; YANG syntax highlighting (tree-sitter-yang @ trislu/tree-sitter-yang).
;
; The language server provides richer, semantic coloring of statement
; arguments when semantic tokens are enabled; these tree-sitter queries give
; a solid baseline (keywords, literals, comments) in the default mode.

(comment) @comment

(quoted_string) @string
(date_str) @string

(integer_value) @number
(decimal_value) @number

(boolean) @boolean

; Statement keywords (module, container, leaf, type, …).
([
  (action_keyword)
  (anydata_keyword)
  (anyxml_keyword)
  (argument_keyword)
  (augment_keyword)
  (base_keyword)
  (belongs_to_keyword)
  (bit_keyword)
  (case_keyword)
  (choice_keyword)
  (config_keyword)
  (contact_keyword)
  (container_keyword)
  (default_keyword)
  (description_keyword)
  (deviate_keyword)
  (deviation_keyword)
  (enum_keyword)
  (error_app_tag_keyword)
  (error_message_keyword)
  (extension_keyword)
  (feature_keyword)
  (fraction_digits_keyword)
  (grouping_keyword)
  (identity_keyword)
  (if_feature_keyword)
  (import_keyword)
  (include_keyword)
  (input_keyword)
  (key_keyword)
  (leaf_keyword)
  (leaf_list_keyword)
  (length_keyword)
  (list_keyword)
  (mandatory_keyword)
  (max_elements_keyword)
  (min_elements_keyword)
  (modifier_keyword)
  (module_keyword)
  (must_keyword)
  (namespace_keyword)
  (notification_keyword)
  (ordered_by_keyword)
  (organization_keyword)
  (output_keyword)
  (path_keyword)
  (pattern_keyword)
  (position_keyword)
  (prefix_keyword)
  (presence_keyword)
  (range_keyword)
  (reference_keyword)
  (refine_keyword)
  (require_instance_keyword)
  (revision_date_keyword)
  (revision_keyword)
  (rpc_keyword)
  (status_keyword)
  (submodule_keyword)
  (type_keyword)
  (typedef_keyword)
  (unique_keyword)
  (units_keyword)
  (uses_keyword)
  (value_keyword)
  (when_keyword)
  (yang_version_keyword)
  (yin_element_keyword)
] @keyword)

; deviate action keywords are literal tokens in this grammar.
[
  "add"
  "delete"
  "not-supported"
] @keyword
