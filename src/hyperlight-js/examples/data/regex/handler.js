function parseUrl(url) {
    let re = /^(?<href>(?:(?<protocol>https?)\:)\/\/(?<host>(?<hostname>[^:\/?#]*)(?:\:(?<port>[0-9]+))?)(?<pathname>[\/]{0,1}[^?#]*)(?:\?(?<search>[^#]*)|)(?:#(?<hash>.*)|))$/;
    let m = re.exec(url);
    return m?.groups;
}

function handler({ url }) {
    return parseUrl(url);
}

export { handler };
