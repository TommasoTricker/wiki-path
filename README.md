Finds the sequence of linked Wikipedia articles required to navigate from one article to another.

Example usage:
```shell
go install github.com/TommasoTricker/wiki-path@latest
wiki-path Donald_Trump Ziggurat # final part of the url
```

Output:
```
Path: [Donald_Trump Iran Ziggurat]
Length: 3
Took 12m24.7296545s
```

Run `wiki-path -h` for more options.
