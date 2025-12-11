package tree_sitter_twinkle_test

import (
	"testing"

	tree_sitter "github.com/tree-sitter/go-tree-sitter"
	tree_sitter_twinkle "github.com/tree-sitter/tree-sitter-twinkle/bindings/go"
)

func TestCanLoadGrammar(t *testing.T) {
	language := tree_sitter.NewLanguage(tree_sitter_twinkle.Language())
	if language == nil {
		t.Errorf("Error loading Twinkle grammar")
	}
}
