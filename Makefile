TOOL := "jimmystewpot/traceroute"
INTERACTIVE := $(shell [ -t 0 ] && echo 1)
TEST_DIRS := ./...
REPORTS_DIR := ci

test-all: deps lint test

deps:
	@echo ""
	@echo "***** Installing dependencies for ${TOOL} *****"
	go clean --cache
	go install github.com/golangci/golangci-lint/cmd/golangci-lint@v1.55.2

lint: deps
	@echo ""
	@echo "***** linting ${TOOL} with golangci-lint *****"
ifdef INTERACTIVE
	golangci-lint run -v $(TEST_DIRS)
else
	golangci-lint run --out-format checkstyle -v $(TEST_DIRS) 1> $(REPORTS_DIR)/checkstyle-lint.xml
endif
.PHONY: lint

test:
	@echo ""
	@echo "***** Testing ${TOOL} *****"
ifdef INTERACTIVE
	go test -a -v -race $(TEST_DIRS) 1
else
	go test -a -v -race -coverprofile=$(REPORTS_DIR)/coverage.txt -covermode=atomic -json $(TEST_DIRS) 1> $(REPORTS_DIR)/testreport.json
	@echo ""
