package demo;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class ExampleTest {
    @Test
    void originalDecisionCases() {
        Example example = new Example();
        assertEquals(0, example.score(true, true, new int[] {1, 2}));
        assertEquals(0, example.score(false, false, new int[0]));
        assertEquals(2, example.nested(true, true, new int[] {1, 2}));
        assertEquals(2, example.nested(false, false, new int[0]));
        assertEquals(1, example.switched(1));
        assertEquals(2, example.switched(0));
    }

    @Test
    void anonymousAndLocalMethodsDoNotBecomeEntries() {
        assertEquals(1, new Example().outer());
    }

    @Test
    void commentsAndStringsAreStable() {
        assertEquals("if && || ? case default catch".length(), new Example().stable());
    }

    @Test
    void oneOverloadAndNestedBinaryNamesHaveCoverage() {
        assertEquals(3, new Example().overloaded(3));
        assertEquals(1, new Example.Inner().tick());
        assertEquals(1, Example.Mode.READY.code());
        assertEquals(2, new Example.Value(2).positive());
    }
}
