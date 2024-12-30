package main

import (
	"flag"
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
	HourSecs = 3600

	AnonRateLimit = 500  // https://api.wikimedia.org/wiki/Rate_limits#Anonymous_requests
	ApiRateLimit  = 5000 // https://api.wikimedia.org/wiki/Rate_limits#Personal_requests

	AnonArticleUrl = "https://en.wikipedia.org/wiki/"
	ApiArticleUrl  = "https://en.wikipedia.org/w/rest.php/v1/page/%s/html"
)

type Node struct {
	Parent *Node
	Value  string
}

type Result struct {
	Node *Node
	Err  error
}

type ScanArticleArgs struct {
	End         string
	Visited     *map[string]struct{}
	Node        *Node
	Mu          *sync.Mutex
	PrevRequest *time.Time
	RateLimitMu *sync.Mutex
	Done        chan Result
	Verbose     bool
	ApiToken    string
	ReqWait     time.Duration
	Auth        bool
	Prefix      string
}

func scanArticle(a ScanArticleArgs) {
	if a.Verbose {
		fmt.Println(a.Node.Value)
	}

	var url string

	if a.Auth {
		url = fmt.Sprintf(ApiArticleUrl, a.Node.Value)
	} else {
		url = AnonArticleUrl + a.Node.Value
	}

	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		a.Done <- Result{nil, err}
		return
	}

	if a.Auth {
		req.Header.Add("Authorization", "Bearer "+a.ApiToken)
	}

	a.RateLimitMu.Lock()
	time.Sleep(a.ReqWait - time.Since(*a.PrevRequest))
	(*a.PrevRequest) = time.Now()
	a.RateLimitMu.Unlock()

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		a.Done <- Result{nil, err}
		return
	}

	doc, err := html.Parse(resp.Body)
	if err != nil {
		resp.Body.Close()
		a.Done <- Result{nil, err}
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

					if strings.HasPrefix(href, a.Prefix) {
						name := strings.TrimPrefix(href, a.Prefix)
						if idx := strings.Index(name, "#"); idx != -1 {
							name = name[:idx]
						}

						if name != "Main_Page" && !strings.Contains(name, ":") {
							newNode := &Node{a.Node, name}

							a.Mu.Lock()

							if name == a.End {
								a.Done <- Result{newNode, nil}
								return
							} else if _, ok := (*a.Visited)[name]; !ok {
								(*a.Visited)[name] = struct{}{}

								newArgs := a
								newArgs.Node = newNode
								go scanArticle(newArgs)
							}

							a.Mu.Unlock()
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

func findPath(start string, end string, verbose bool, apiToken string) Result {
	visited := make(map[string]struct{})
	visited[start] = struct{}{}

	node := Node{nil, start}

	done := make(chan Result)
	mu := sync.Mutex{}
	rateLimitMu := sync.Mutex{}

	var rateLimit int
	var auth bool
	var prefix string

	if apiToken != "" {
		rateLimit = ApiRateLimit
		auth = true
		prefix = "./"
	} else {
		rateLimit = AnonRateLimit
		auth = false
		prefix = "/wiki/"
	}

	reqWait := time.Duration((float32(HourSecs) / float32(rateLimit)) * float32(time.Second))

	prevRequest := time.Now().Add(-reqWait)

	a := ScanArticleArgs{end, &visited, &node, &mu, &prevRequest, &rateLimitMu, done, verbose, apiToken, reqWait, auth, prefix}

	go scanArticle(a)

	result := <-done

	return result
}

func main() {
	flag.Usage = func() {
		fmt.Fprintln(os.Stderr, "Usage: wiki-path [options] <start> <end>")
		fmt.Fprintln(os.Stderr, "Options:")

		flag.PrintDefaults()
	}

	help := flag.Bool("h", false, "Show help message")
	verbose := flag.Bool("v", false, "Print all articles that will be visited")
	apiToken := flag.String("t", "", "(Optional) API token for Wikipedia to increase the rate limit (https://api.wikimedia.org/wiki/Authentication#Personal_API_tokens)")

	flag.Parse()

	if *help {
		flag.Usage()
		os.Exit(0)
	}

	args := flag.Args()

	startTime := time.Now()
	result := findPath(args[0], args[1], *verbose, *apiToken)
	endTime := time.Now()

	elapsed := endTime.Sub(startTime)

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

	fmt.Printf("Path: %s\n", path)
	fmt.Printf("Length: %d\n", len(path))
	fmt.Printf("Took %s\n", elapsed)
}
