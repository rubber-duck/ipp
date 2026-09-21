use syn::Ident;

pub(super) fn snake_case(name: &Ident) -> Ident {
    let chars: Vec<_> = name.to_string().chars().collect();
    let mut snake = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            let acronym_boundary = index > 0
                && chars[index - 1].is_uppercase()
                && chars.get(index + 1).is_some_and(|next| next.is_lowercase());
            if index > 0 && (chars[index - 1].is_lowercase() || acronym_boundary) {
                snake.push('_');
            }
            snake.extend(ch.to_lowercase());
        } else {
            snake.push(ch);
        }
    }
    Ident::new(&snake, name.span())
}

pub(super) fn upper_snake_case(name: &Ident) -> Ident {
    Ident::new(&snake_case(name).to_string().to_uppercase(), name.span())
}
