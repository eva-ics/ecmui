cargo clippy -- -W clippy::all -W clippy::pedantic \
  -A clippy::used-underscore-binding \
  -A clippy::doc_markdown \
  -A clippy::needless_pass_by_value \
  -A clippy::must_use_candidate \
  -A clippy::return_self_not_must_use \
  -A clippy::missing_errors_doc \
  -A clippy::single_match \
  -A clippy::uninlined_format_args \
  -A clippy::no_effect_underscore_binding
  