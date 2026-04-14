use ai_agent::engine::preprocess_compat_source;
use std::path::Path;

fn main() {
    let source = r#"
            async function main() {
              const events = [];
              const resources = [
                { async [Symbol.asyncDispose]() { events.push('dispose-1'); } },
                { async [Symbol.asyncDispose]() { events.push('dispose-2'); } }
              ];
              for (await using x of resources) {
                events.push(x === resources[0] ? 'body-1' : 'body-2');
              }
              return events.join(',');
            }
            main().then((value) => print(value));
    "#;
    
    match preprocess_compat_source(source, None, false, false) {
        Ok(rewritten) => println!("{}", rewritten),
        Err(e) => eprintln!("Error: {:?}", e),
    }
}
