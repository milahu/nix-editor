use crate::parse::{findattr, getcfgbase, getkey};
use failure::Fail;
use rnix::{self, SyntaxKind, SyntaxNode};

#[derive(Fail, Debug)]
pub enum WriteError {
    #[fail(display = "Write Error: Error while parsing.")]
    ParseError,
    #[fail(display = "Write Error: No attributes.")]
    NoAttr,
    #[fail(display = "Write Error: Error with array.")]
    ArrayError,
}

pub fn write(f: &str, query: &str, val: &str) -> Result<String, WriteError> {
    let ast = rnix::parse(&f);
    let configbase = match getcfgbase(&ast.node()) {
        Some(x) => x,
        None => {
            return Err(WriteError::ParseError);
        }
    };
    let outnode = match findattr(&configbase, &query) {
        Some(x) => modvalue(&x, &val).unwrap(),
        None => {
            let mut y = query.split(".").collect::<Vec<_>>();
            y.pop();
            let x = findattrset(&configbase, &y.join("."), 0);
            match x {
                Some((base, v, spaces)) => addvalue(&base, &format!("{}{}", " ".repeat(spaces), &query[v.len()+1..]), &val),
                None => addvalue(&configbase, &query, &val),
            }
        }
    };
    Ok(outnode.to_string())
}

fn addvalue(configbase: &SyntaxNode, query: &str, val: &str) -> SyntaxNode {
    let mut index = configbase.green().children().len() - 2;
    // To find a better index for insertion, first find a matching node, then find the next newline token, after that, insert
    match matchval(&configbase, &query, query.split(".").count()) {
        Some(x) => {
            let i = configbase
                .green()
                .children()
                .position(|y| match y.into_node() {
                    Some(y) => y.to_owned() == x.green().to_owned(),
                    None => false,
                })
                .unwrap();
            let configafter = &configbase.green().children().collect::<Vec<_>>()[i..];
            for child in configafter {
                match child.as_token() {
                    Some(x) => {
                        if x.text().contains("\n") {
                            let cas = configafter.to_vec();
                            index = i + cas
                                .iter()
                                .position(|y| match y.as_token() {
                                    Some(t) => t == x,
                                    None => false,
                                })
                                .unwrap();
                            break;
                        }
                    }
                    None => {}
                }
            }
        }
        None => {}
    }
    let input = rnix::parse(format!("\n  {} = {};", &query, &val).as_str())
        .node()
        .green()
        .to_owned();
    let new = configbase
        .green()
        .insert_child(index, rnix::NodeOrToken::Node(input));
    let replace = configbase.replace_with(new.clone());
    rnix::parse(&replace.to_string()).node()
}

fn findattrset(configbase: &SyntaxNode, name: &str, spaces: usize) -> Option<(SyntaxNode, String, usize)> {
    for child in configbase.children() {
        if child.kind() == SyntaxKind::NODE_KEY_VALUE {
            // Now we have to read all the indent values from the key
            for subchild in child.children() {
                if subchild.kind() == SyntaxKind::NODE_KEY {
                    // We have a key, now we need to check if it's the one we're looking for
                    let key = getkey(&subchild);
                    let qkey = name
                        .split('.')
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>();
                    if qkey == key {
                        // We have key, now lets find the attrset
                        for possibleset in child.children() {
                            if possibleset.kind() == SyntaxKind::NODE_ATTR_SET {
                                return Some((possibleset, name.to_string(), spaces+2));
                            }
                        }
                        return None;
                    } else if qkey.len() > key.len() {
                        // We have a subkey, so we need to recurse
                        if key == qkey[0..key.len()] {
                            // We have a subkey, so we need to recurse
                            let subkey = &qkey[key.len()..].join(".").to_string();
                            let newbase = getcfgbase(&child).unwrap();
                            let subattr = findattrset(&newbase, subkey, spaces+2);
                            if subattr.is_some() {
                                return subattr;
                            }
                        }
                    }
                }
            }
        }
    }
    return None;
}

fn matchval(configbase: &SyntaxNode, query: &str, acc: usize) -> Option<SyntaxNode> {
    let qvec = &query
        .split(".")
        .map(|s| s.to_string())
        .collect::<Vec<String>>();
    let q = &qvec[..acc];
    for child in configbase.children() {
        if child.kind() == SyntaxKind::NODE_KEY_VALUE {
            for subchild in child.children() {
                if subchild.kind() == SyntaxKind::NODE_KEY {
                    let key = getkey(&subchild);
                    if key.len() >= q.len() {
                        if &key[..q.len()] == q {
                            return Some(child.clone());
                        }
                    }
                }
            }
        }
    }
    if acc == 1 {
        return None;
    } else {
        matchval(configbase, query, acc - 1)
    }
}

fn modvalue(node: &SyntaxNode, val: &str) -> Option<SyntaxNode> {
    // First find the IDENT node
    for child in node.children() {
        if child.kind() != SyntaxKind::NODE_KEY {
            let c = &child.clone();
            let input = val.to_string();
            let rep = &rnix::parse(&input)
                .node()
                .children()
                .collect::<Vec<SyntaxNode>>()[0];
            let index = node
                .green()
                .children()
                .position(|y| match y.into_node() {
                    Some(y) => y.to_owned() == c.green().to_owned(),
                    None => false,
                })
                .unwrap();
            let replaced = node
                .green()
                .replace_child(index, rnix::NodeOrToken::Node(rep.green().to_owned()));
            let out = node.replace_with(replaced);
            let rnode = rnix::parse(&out.to_string()).node();
            return Some(rnode);
        }
    }
    return None;
}

pub fn addtoarr(f: &str, query: &str, items: Vec<String>) -> Result<String, WriteError> {
    let ast = rnix::parse(&f);
    let configbase = match getcfgbase(&ast.node()) {
        Some(x) => x,
        None => return Err(WriteError::ParseError),
    };
    let outnode = match findattr(&configbase, &query) {
        Some(x) => match addtoarr_aux(&x, items) {
            Some(x) => x,
            None => return Err(WriteError::ArrayError),
        },
        // If no arrtibute is found, create a new one
        None => {
            let newval = addvalue(&configbase, query, "[\n  ]");
            return addtoarr(&newval.to_string(), query, items);
        }
    };
    Ok(outnode.to_string())
}

fn addtoarr_aux(node: &SyntaxNode, items: Vec<String>) -> Option<SyntaxNode> {
    for child in node.children() {
        if child.kind() == rnix::SyntaxKind::NODE_WITH {
            return addtoarr_aux(&child, items.clone());
        }
        if child.kind() == SyntaxKind::NODE_LIST {
            let mut green = child.green().to_owned();

            for elem in items {
                let mut i = 0;
                for c in green.children() {
                    if c.to_string() == "]" {
                        if green.children().collect::<Vec<_>>()[i - 1]
                            .as_token()
                            .unwrap()
                            .to_string()
                            .contains("\n")
                        {
                            i -= 1;
                        }
                        green = green.insert_child(
                            i,
                            rnix::NodeOrToken::Node(
                                rnix::parse(&format!("\n{}{}", " ".repeat(4), elem))
                                    .node()
                                    .green()
                                    .to_owned(),
                            ),
                        );
                        break;
                    }
                    i += 1;
                }
            }

            let index = match node.green().children().position(|x| match x.into_node() {
                Some(x) => x.to_owned() == child.green().to_owned(),
                None => false,
            }) {
                Some(x) => x,
                None => return None,
            };

            let replace = node
                .green()
                .replace_child(index, rnix::NodeOrToken::Node(green));
            let out = node.replace_with(replace);
            let output = rnix::parse(&out.to_string()).node();
            return Some(output);
        }
    }
    return None;
}

pub fn rmarr(f: &str, query: &str, items: Vec<String>) -> Result<String, WriteError> {
    let ast = rnix::parse(&f);
    let configbase = match getcfgbase(&ast.node()) {
        Some(x) => x,
        None => return Err(WriteError::ParseError),
    };
    let outnode = match findattr(&configbase, &query) {
        Some(x) => match rmarr_aux(&x, items) {
            Some(x) => x,
            None => return Err(WriteError::ArrayError),
        },
        None => return Err(WriteError::NoAttr),
    };
    Ok(outnode.to_string())
}

fn rmarr_aux(node: &SyntaxNode, items: Vec<String>) -> Option<SyntaxNode> {
    for child in node.children() {
        if child.kind() == rnix::SyntaxKind::NODE_WITH {
            return rmarr_aux(&child, items.clone());
        }
        if child.kind() == SyntaxKind::NODE_LIST {
            let green = child.green().to_owned();
            let mut idx = vec![];
            for elem in green.children() {
                if elem.as_node() != None && items.contains(&elem.to_string()) {
                    let index = match green.children().position(|x| match x.into_node() {
                        Some(x) => x.to_owned() == elem.as_node().unwrap().to_owned().to_owned(),
                        None => false,
                    }) {
                        Some(x) => x,
                        None => return None,
                    };
                    idx.push(index)
                }
            }
            let mut acc = 0;
            let mut replace = green.to_owned();

            for i in idx {
                replace = replace.remove_child(i - acc);
                let mut v = vec![];
                for c in replace.children() {
                    v.push(c);
                }
                match v.get(i - acc - 1).unwrap().as_token() {
                    Some(x) => {
                        if x.to_string().contains("\n") {
                            replace = replace.remove_child(i - acc - 1);
                            acc += 1;
                        }
                    }
                    None => {}
                }
                acc += 1;
            }
            let out = child.replace_with(replace);

            let output = rnix::parse(&out.to_string()).node();
            return Some(output);
        }
    }
    return None;
}

pub fn deref(f: &str, query: &str) -> Result<String, WriteError> {
    let ast = rnix::parse(&f);
    let configbase = match getcfgbase(&ast.node()) {
        Some(x) => x,
        None => return Err(WriteError::ParseError),
    };
    let outnode = match deref_aux(&configbase, &query) {
        Some(x) => x,
        None => return Err(WriteError::NoAttr),
    };
    Ok(outnode.to_string())
}

fn deref_aux(configbase: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    for child in configbase.children() {
        if child.kind() == SyntaxKind::NODE_KEY_VALUE {
            // Now we have to read all the indent values from the key
            for subchild in child.children() {
                if subchild.kind() == SyntaxKind::NODE_KEY {
                    // We have a key, now we need to check if it's the one we're looking for
                    let key = getkey(&subchild);
                    let qkey = name
                        .split(".")
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>();
                    if qkey == key {
                        let index =
                            match configbase
                                .green()
                                .children()
                                .position(|x| match x.into_node() {
                                    Some(x) => x.to_owned() == child.green().to_owned(),
                                    None => false,
                                }) {
                                Some(x) => x,
                                None => return None,
                            };
                        let mut del = configbase.green().remove_child(index);

                        // Remove leading newline if it still exists
                        if del.children().collect::<Vec<_>>()[index]
                            .to_string()
                            .contains("\n")
                        {
                            del = del.remove_child(index);
                        }
                        let out = configbase.replace_with(del);
                        return Some(rnix::parse(&out.to_string()).node());
                    } else if qkey.len() > key.len() {
                        // We have a subkey, so we need to recurse
                        if key == qkey[0..key.len()] {
                            // We have a subkey, so we need to recurse
                            let subkey = &qkey[key.len()..].join(".").to_string();
                            let newbase = getcfgbase(&child).unwrap();
                            let subattr = deref_aux(&newbase, &subkey);
                            if subattr.is_some() {
                                return subattr;
                            }
                        }
                    }
                }
            }
        }
    }
    return None;
}
