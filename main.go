package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
	"slices"
	"strings"
	"sync"
	"time"

	"golang.org/x/net/html"
)

const (
	RateLimit = time.Duration(7.2 * float32(time.Second)) // https://api.wikimedia.org/wiki/Rate_limits
	BaseUrl   = "https://en.wikipedia.org/wiki/"
)

type Node struct {
	Parent *Node
	Value  string
}

type Result struct {
	Node *Node
	Err  error
}

func scanPath(end string, visited *map[string]struct{}, node *Node, mu *sync.Mutex, prevRequest *time.Time, rateLimitMu *sync.Mutex, done chan Result) {
	fmt.Println(node.Value)

	rateLimitMu.Lock()
	time.Sleep(RateLimit - time.Since(*prevRequest))
	(*prevRequest) = time.Now()
	rateLimitMu.Unlock()

	resp, err := http.Get(BaseUrl + node.Value)
	if err != nil {
		done <- Result{nil, err}
		return
	}

	doc, err := html.Parse(resp.Body)
	if err != nil {
		resp.Body.Close()
		done <- Result{nil, err}
		return
	}

	resp.Body.Close()

	htmlNodes := []*html.Node{doc}

	for len(htmlNodes) > 0 {
		htmlNode := htmlNodes[len(htmlNodes)-1]
		htmlNodes = htmlNodes[:len(htmlNodes)-1]

		if htmlNode.Type == html.ElementNode && htmlNode.Data == "a" {
			for _, attr := range htmlNode.Attr {
				if attr.Key == "href" {
					href := attr.Val

					if strings.HasPrefix(href, "/wiki/") {
						name := strings.TrimPrefix(href, "/wiki/")
						if idx := strings.Index(name, "#"); idx != -1 {
							name = name[:idx]
						}

						if name != "Main_Page" && !strings.Contains(name, ":") {
							newNode := Node{node, name}

							mu.Lock()

							if name == end {
								done <- Result{&newNode, nil}
								return
							} else if _, ok := (*visited)[name]; !ok {
								(*visited)[name] = struct{}{}
								go scanPath(end, visited, &newNode, mu, prevRequest, rateLimitMu, done)
							}

							mu.Unlock()
						}
					}
				}
			}
		}

		for child := htmlNode.FirstChild; child != nil; child = child.NextSibling {
			htmlNodes = append(htmlNodes, child)
		}
	}
}

func findPath(start string, end string) Result {
	visited := make(map[string]struct{})
	visited[start] = struct{}{}

	node := Node{nil, start}

	done := make(chan Result)
	mu := sync.Mutex{}
	rateLimitMu := sync.Mutex{}

	prevRequest := time.Now().Add(-RateLimit)

	go scanPath(end, &visited, &node, &mu, &prevRequest, &rateLimitMu, done)

	result := <-done

	return result
}

func main() {
	result := findPath(os.Args[1], os.Args[2])

	node := result.Node
	err := result.Err

	if err != nil {
		log.Fatal(result.Err)
	}

	path := []string{}

	for node != nil {
		path = append(path, node.Value)
		node = node.Parent
	}

	slices.Reverse(path)

	fmt.Println(path)
	fmt.Println(len(path))
}
